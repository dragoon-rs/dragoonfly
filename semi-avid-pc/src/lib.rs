use std::ops::{Div, Mul};

use ark_ec::pairing::Pairing;
use ark_ff::{BigInteger, Field, PrimeField};
use ark_poly::DenseUVPolynomial;
use ark_poly_commit::kzg10::{Commitment, Powers, Randomness, KZG10};

use serde::{Deserialize, Serialize};

mod ark_ser;
mod field;
pub mod setup;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shard {
    /// the number of required shards for reconstruction
    k: u32,
    /// the index of the current shard
    i: u32,
    /// the hash of the original data
    hash: Vec<u8>,
    /// the shard bytes
    bytes: Vec<u8>,
    /// the data size
    size: usize,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block<E: Pairing> {
    shard: Shard,
    #[serde(with = "ark_ser")]
    commit: Vec<Commitment<E>>,
    m: usize,
}

#[allow(clippy::type_complexity)]
pub fn commit<E, P>(
    powers: &Powers<E>,
    polynomials: &[P],
) -> Result<(Vec<Commitment<E>>, Vec<Randomness<E::ScalarField, P>>), ark_poly_commit::Error>
where
    E: Pairing,
    P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
    for<'a, 'b> &'a P: Div<&'b P, Output = P>,
{
    let mut commits = Vec::new();
    let mut randomnesses = Vec::new();
    for polynomial in polynomials {
        let (commit, randomness) = KZG10::<E, P>::commit(powers, polynomial, None, None)?;
        commits.push(commit);
        randomnesses.push(randomness);
    }

    Ok((commits, randomnesses))
}

pub fn prove<E, P>(
    commits: Vec<Commitment<E>>,
    hash: [u8; 32],
    nb_bytes: usize,
    polynomials: Vec<P>,
    points: &[E::ScalarField],
) -> Result<Vec<Block<E>>, ark_poly_commit::Error>
where
    E: Pairing,
    P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
    for<'a, 'b> &'a P: Div<&'b P, Output = P>,
{
    let k = polynomials[0].coeffs().len();

    let evaluations = points
        .iter()
        .map(|point| polynomials.iter().map(|p| p.evaluate(point)).collect())
        .collect::<Vec<Vec<E::ScalarField>>>();

    let mut proofs = Vec::new();
    for (i, row) in evaluations.iter().enumerate() {
        let mut shard = vec![];
        for r in row {
            shard.append(&mut r.into_bigint().to_bytes_le());
        }

        proofs.push(Block {
            shard: Shard {
                k: k as u32,
                i: i as u32,
                hash: hash.to_vec(),
                bytes: shard,
                size: nb_bytes,
            },
            commit: commits.clone(),
            m: polynomials.len(),
        })
    }

    Ok(proofs)
}

pub fn verify<E, P>(block: &Block<E>, verifier_key: &Powers<E>) -> bool
where
    E: Pairing,
    P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
    for<'a, 'b> &'a P: Div<&'b P, Output = P>,
{
    let alpha = E::ScalarField::from_le_bytes_mod_order(&[block.shard.i as u8]);

    let mut elements = Vec::new();
    for chunk in block
        .shard
        .bytes
        .chunks((E::ScalarField::MODULUS_BIT_SIZE as usize) / 8 + 1)
    {
        elements.push(E::ScalarField::from_le_bytes_mod_order(chunk));
    }
    let polynomial = P::from_coefficients_vec(elements);
    let (commit, _) = KZG10::<E, P>::commit(verifier_key, &polynomial, None, None).unwrap();

    Into::<E::G1>::into(commit.0)
        == block
            .commit
            .iter()
            .enumerate()
            .map(|(j, c)| {
                let commit: E::G1 = c.0.into();
                commit.mul(alpha.pow([j as u64]))
            })
            .sum()
}

pub fn batch_verify<E, P>(blocks: &[Block<E>], verifier_key: &Powers<E>) -> bool
where
    E: Pairing,
    P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
    for<'a, 'b> &'a P: Div<&'b P, Output = P>,
{
    for block in blocks {
        if !verify(block, verifier_key) {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use std::ops::{Div, Mul};

    use ark_bls12_381::Bls12_381;
    use ark_ec::pairing::Pairing;
    use ark_ff::{Field, PrimeField};
    use ark_poly::{univariate::DensePolynomial, DenseUVPolynomial};
    use ark_poly_commit::kzg10::{Commitment, Powers, KZG10};
    use ark_std::test_rng;
    use rs_merkle::algorithms::Sha256;
    use rs_merkle::Hasher;

    use crate::{batch_verify, commit, field, prove, setup::trim, verify, Block};

    type UniPoly381 = DensePolynomial<<Bls12_381 as Pairing>::ScalarField>;

    fn bytes<E: Pairing>(k: usize, nb_polynomials: usize) -> Vec<u8> {
        let nb_bytes = k * nb_polynomials * (E::ScalarField::MODULUS_BIT_SIZE as usize / 8);
        include_bytes!("../../res/dragoon_133x133.png")[0..nb_bytes].to_vec()
    }

    fn test_setup<E, P>(
        bytes: &[u8],
        k: usize,
        n: usize,
    ) -> Result<(Vec<Block<E>>, Powers<E>), ark_poly_commit::Error>
    where
        E: Pairing,
        P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        let degree = bytes.len() / (E::ScalarField::MODULUS_BIT_SIZE as usize / 8);

        let rng = &mut test_rng();

        let params = KZG10::<E, P>::setup(degree, false, rng)?;
        let (powers, _) = trim(params, degree)?;

        let elements = field::split_data_into_field_elements::<E>(bytes, k);
        let nb_polynomials = elements.len() / k;
        let polynomials = field::build_interleaved_polynomials::<E, P>(&elements, nb_polynomials);

        let polynomials_to_commit = (0..polynomials[0].coeffs().len())
            .map(|i| P::from_coefficients_vec(polynomials.iter().map(|p| p.coeffs()[i]).collect()))
            .collect::<Vec<P>>();

        let (commits, _) = commit(&powers, &polynomials_to_commit).unwrap();

        let points: Vec<E::ScalarField> = (0..n)
            .map(|i| E::ScalarField::from_le_bytes_mod_order(&[i as u8]))
            .collect();

        let hash = Sha256::hash(bytes);

        let blocks = prove::<E, P>(commits, hash, bytes.len(), polynomials, &points)
            .expect("Semi-AVID-PR proof failed");

        Ok((blocks, powers))
    }

    fn verify_template<E, P>(bytes: &[u8], k: usize, n: usize) -> Result<(), ark_poly_commit::Error>
    where
        E: Pairing,
        P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        let (blocks, verifier_key) =
            test_setup::<E, P>(bytes, k, n).expect("proof failed for bls12-381");

        for block in &blocks {
            assert!(verify::<E, P>(block, &verifier_key));
        }

        assert!(batch_verify(&blocks[1..3], &verifier_key));

        Ok(())
    }

    #[test]
    fn verify_2() {
        let bytes = bytes::<Bls12_381>(4, 2);
        verify_template::<Bls12_381, UniPoly381>(&bytes, 4, 6)
            .expect("verification failed for bls12-381");
        verify_template::<Bls12_381, UniPoly381>(&bytes[0..(bytes.len() - 10)], 4, 6)
            .expect("verification failed for bls12-381 with padding");
    }

    #[test]
    fn verify_4() {
        let bytes = bytes::<Bls12_381>(4, 4);
        verify_template::<Bls12_381, UniPoly381>(&bytes, 4, 6)
            .expect("verification failed for bls12-381");
        verify_template::<Bls12_381, UniPoly381>(&bytes[0..(bytes.len() - 10)], 4, 6)
            .expect("verification failed for bls12-381 with padding");
    }

    #[test]
    fn verify_6() {
        let bytes = bytes::<Bls12_381>(4, 6);
        verify_template::<Bls12_381, UniPoly381>(&bytes, 4, 6)
            .expect("verification failed for bls12-381");
        verify_template::<Bls12_381, UniPoly381>(&bytes[0..(bytes.len() - 10)], 4, 6)
            .expect("verification failed for bls12-381 with padding");
    }

    #[ignore = "Semi-AVID-PR does not support large padding"]
    #[test]
    fn verify_with_large_padding_2() {
        let bytes = bytes::<Bls12_381>(4, 2);
        verify_template::<Bls12_381, UniPoly381>(&bytes[0..(bytes.len() - 33)], 4, 6)
            .expect("verification failed for bls12-381 with padding");
    }

    #[ignore = "Semi-AVID-PR does not support large padding"]
    #[test]
    fn verify_with_large_padding_4() {
        let bytes = bytes::<Bls12_381>(4, 4);
        verify_template::<Bls12_381, UniPoly381>(&bytes[0..(bytes.len() - 33)], 4, 6)
            .expect("verification failed for bls12-381 with padding");
    }

    #[ignore = "Semi-AVID-PR does not support large padding"]
    #[test]
    fn verify_with_large_padding_6() {
        let bytes = bytes::<Bls12_381>(4, 6);
        verify_template::<Bls12_381, UniPoly381>(&bytes[0..(bytes.len() - 33)], 4, 6)
            .expect("verification failed for bls12-381 with padding");
    }

    fn verify_with_errors_template<E, P>(
        bytes: &[u8],
        k: usize,
        n: usize,
    ) -> Result<(), ark_poly_commit::Error>
    where
        E: Pairing,
        P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        let (blocks, verifier_key) =
            test_setup::<E, P>(bytes, k, n).expect("proof failed for bls12-381");

        for block in &blocks {
            assert!(verify::<E, P>(block, &verifier_key));
        }

        let mut corrupted_block = blocks[0].clone();
        // modify a field in the struct b to corrupt the block proof without corrupting the data serialization
        let a = E::ScalarField::from_le_bytes_mod_order(&[123]);
        let mut commits: Vec<E::G1> = corrupted_block.commit.iter().map(|c| c.0.into()).collect();
        commits[0] = commits[0].mul(a.pow([4321_u64]));
        corrupted_block.commit = commits.iter().map(|&c| Commitment(c.into())).collect();

        assert!(!verify::<E, P>(&corrupted_block, &verifier_key));

        // let's build some blocks containing errors
        let mut blocks_with_errors = Vec::new();

        let b3 = blocks.get(3).unwrap();
        blocks_with_errors.push(Block {
            shard: b3.shard.clone(),
            commit: b3.commit.clone(),
            m: b3.m,
        });
        assert!(batch_verify(blocks_with_errors.as_slice(), &verifier_key));

        blocks_with_errors.push(corrupted_block);
        assert!(!batch_verify(blocks_with_errors.as_slice(), &verifier_key));

        Ok(())
    }

    #[test]
    fn verify_with_errors_2() {
        let bytes = bytes::<Bls12_381>(4, 2);
        verify_with_errors_template::<Bls12_381, UniPoly381>(&bytes, 4, 6)
            .expect("verification failed for bls12-381");
        verify_with_errors_template::<Bls12_381, UniPoly381>(&bytes[0..(bytes.len() - 10)], 4, 6)
            .expect("verification failed for bls12-381 with padding");
    }

    #[test]
    fn verify_with_errors_4() {
        let bytes = bytes::<Bls12_381>(4, 4);
        verify_with_errors_template::<Bls12_381, UniPoly381>(&bytes, 4, 6)
            .expect("verification failed for bls12-381");
        verify_with_errors_template::<Bls12_381, UniPoly381>(&bytes[0..(bytes.len() - 10)], 4, 6)
            .expect("verification failed for bls12-381 with padding");
    }

    #[test]
    fn verify_with_errors_6() {
        let bytes = bytes::<Bls12_381>(4, 6);
        verify_with_errors_template::<Bls12_381, UniPoly381>(&bytes, 4, 6)
            .expect("verification failed for bls12-381");
        verify_with_errors_template::<Bls12_381, UniPoly381>(&bytes[0..(bytes.len() - 10)], 4, 6)
            .expect("verification failed for bls12-381 with padding");
    }

    #[test]
    fn serde_json() {
        let (blocks, _) = test_setup::<Bls12_381, UniPoly381>(&bytes::<Bls12_381>(8, 2), 8, 16)
            .expect("proof failed for bls12-381");
        let block = blocks.get(0).unwrap();

        let ser = serde_json::to_string(&block).unwrap();

        let deser_block: Block<Bls12_381> = serde_json::from_str(ser.as_str()).unwrap();

        assert_eq!(&deser_block, block)
    }

    #[test]
    fn serde_bincode() {
        let (blocks, _) = test_setup::<Bls12_381, UniPoly381>(&bytes::<Bls12_381>(8, 2), 8, 16)
            .expect("proof failed for bls12-381");
        let block = blocks.get(0).unwrap();

        let ser = bincode::serialize(&block).unwrap();

        let deser_block: Block<Bls12_381> = bincode::deserialize(&ser).unwrap();

        assert_eq!(&deser_block, block)
    }
}
