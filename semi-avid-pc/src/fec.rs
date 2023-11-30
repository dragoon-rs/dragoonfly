use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use reed_solomon_erasure::{Error, Field, ReedSolomonNonSystematic};

#[derive(Debug, Default, Clone, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct LinearCombinationElement {
    pub index: u32,
    pub weight: u32,
}

#[derive(Debug, Default, Clone, PartialEq, CanonicalSerialize, CanonicalDeserialize)]
pub struct Shard {
    pub k: u32,
    pub linear_combination: Vec<LinearCombinationElement>,
    pub hash: Vec<u8>,
    pub bytes: Vec<u8>,
    pub size: usize,
}

pub fn decode<F: Field>(blocks: Vec<Shard>) -> Result<Vec<u8>, Error> {
    let k = blocks[0].k;
    let n = blocks
        .iter()
        // FIXME: this is incorrect
        .map(|b| b.linear_combination[0].index)
        .max()
        .unwrap_or(0)
        + 1;

    if blocks.len() < k as usize {
        return Err(Error::TooFewShards);
    }

    let mut shards: Vec<Option<Vec<F::Elem>>> = Vec::with_capacity(n as usize);
    shards.resize(n as usize, None);
    for block in &blocks {
        // FIXME: this is incorrect
        shards[block.linear_combination[0].index as usize] = Some(F::deserialize(&block.bytes));
    }

    ReedSolomonNonSystematic::<F>::vandermonde(k as usize, n as usize)?.reconstruct(&mut shards)?;
    let elements: Vec<_> = shards.iter().filter_map(|x| x.clone()).flatten().collect();

    let mut data = F::into_data(elements.as_slice());
    data.resize(blocks[0].size, 0);
    Ok(data)
}

#[cfg(test)]
mod tests {
    use ark_bls12_381::Bls12_381;
    use ark_ec::pairing::Pairing;
    use ark_ff::{BigInteger, PrimeField};
    use reed_solomon_erasure::galois_prime::Field as GF;
    use rs_merkle::algorithms::Sha256;
    use rs_merkle::Hasher;

    use crate::fec::{decode, LinearCombinationElement, Shard};

    const DATA: &[u8] = b"f\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0o\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0o\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0b\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0a\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0r\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0b\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0a\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0z\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";

    const K: usize = 3;

    const SHARDS: [[u32; K]; 7] = [
        [102u32, 111u32, 111u32],
        [298u32, 305u32, 347u32],
        [690u32, 693u32, 827u32],
        [1278u32, 1275u32, 1551u32],
        [2062u32, 2051u32, 2519u32],
        [3042u32, 3021u32, 3731u32],
        [4218u32, 4185u32, 5187u32],
    ];
    const LOST_SHARDS: [usize; 3] = [1, 3, 6];

    fn to_big_int_from_bytes(i: &[u8]) -> <Bls12_381 as Pairing>::ScalarField {
        <Bls12_381 as Pairing>::ScalarField::from_le_bytes_mod_order(i)
    }

    #[test]
    fn decoding() {
        let hash = Sha256::hash(DATA).to_vec();

        let mut shards = SHARDS
            .iter()
            .map(|r| {
                Some(
                    r.iter()
                        .map(|s| to_big_int_from_bytes(&s.to_le_bytes()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        for i in LOST_SHARDS {
            shards[i] = None;
        }

        let mut blocks = Vec::new();
        for (i, shard) in shards.iter().enumerate() {
            if let Some(bytes) = shard {
                let mut shard = vec![];
                for r in bytes {
                    shard.append(&mut r.into_bigint().to_bytes_le());
                }
                blocks.push(Shard {
                    k: K as u32,
                    linear_combination: vec![LinearCombinationElement {
                        index: i as u32,
                        weight: 1,
                    }],
                    hash: hash.clone(),
                    bytes: shard,
                    size: DATA.len(),
                });
            }
        }

        assert_eq!(DATA, decode::<GF>(blocks).unwrap())
    }
}
