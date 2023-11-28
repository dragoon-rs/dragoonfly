use reed_solomon_erasure::{Error, Field, ReedSolomonNonSystematic};

use crate::Shard;

pub fn decode<F: Field>(blocks: Vec<Shard>) -> Result<Vec<u8>, Error> {
    let k = blocks[0].k;
    let n = blocks.iter().map(|block| block.i).max().unwrap_or(0) + 1;

    if blocks.len() < k as usize {
        return Err(Error::TooFewShards);
    }

    let mut shards: Vec<Option<Vec<F::Elem>>> = Vec::with_capacity(n as usize);
    shards.resize(n as usize, None);
    let data_size = blocks[0].size;
    for block in blocks {
        shards[block.i as usize] = Some(F::deserialize(&block.bytes));
    }

    ReedSolomonNonSystematic::<F>::vandermonde(k as usize, n as usize)?.reconstruct(&mut shards)?;
    let elements: Vec<_> = shards.iter().filter_map(|x| x.clone()).flatten().collect();

    let mut data = F::into_data(elements.as_slice());
    data.resize(data_size, 0);
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

    use crate::{fec::decode, Shard};

    const DATA: &[u8] = b"foobarbaz";

    fn bar(i: &[u8]) -> <Bls12_381 as Pairing>::ScalarField {
        <Bls12_381 as Pairing>::ScalarField::from_le_bytes_mod_order(i)
    }

    #[test]
    fn decoding() {
        let hash = Sha256::hash(DATA).to_vec();

        let mut shards = [
            Some([
                bar(&102u32.to_be_bytes()),
                bar(&111u32.to_be_bytes()),
                bar(&111u32.to_be_bytes()),
            ]),
            Some([
                bar(&298u32.to_be_bytes()),
                bar(&305u32.to_be_bytes()),
                bar(&347u32.to_be_bytes()),
            ]),
            Some([
                bar(&690u32.to_be_bytes()),
                bar(&693u32.to_be_bytes()),
                bar(&827u32.to_be_bytes()),
            ]),
            Some([
                bar(&1278u32.to_be_bytes()),
                bar(&1275u32.to_be_bytes()),
                bar(&1551u32.to_be_bytes()),
            ]),
            Some([
                bar(&2062u32.to_be_bytes()),
                bar(&2051u32.to_be_bytes()),
                bar(&2519u32.to_be_bytes()),
            ]),
            Some([
                bar(&3042u32.to_be_bytes()),
                bar(&3021u32.to_be_bytes()),
                bar(&3731u32.to_be_bytes()),
            ]),
            Some([
                bar(&4218u32.to_be_bytes()),
                bar(&4185u32.to_be_bytes()),
                bar(&5187u32.to_be_bytes()),
            ]),
        ];
        for i in [1, 3, 6] {
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
                    k: 3,
                    i: i as u32,
                    hash: hash.clone(),
                    bytes: shard,
                    size: DATA.len(),
                });
            }
        }

        assert_eq!(DATA, decode::<GF>(blocks).unwrap())
    }
}
