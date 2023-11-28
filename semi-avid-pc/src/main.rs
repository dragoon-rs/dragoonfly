use std::io::prelude::*;
use std::ops::Div;
use std::process::exit;
use std::{fs::File, path::PathBuf};

use ark_bls12_381::Bls12_381;
use ark_ec::pairing::Pairing;
use ark_ff::PrimeField;
use ark_poly::{univariate::DensePolynomial, DenseUVPolynomial};
use ark_poly_commit::kzg10::{Powers, KZG10};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Compress, Validate};
use ark_std::test_rng;
use rs_merkle::algorithms::Sha256;
use rs_merkle::Hasher;

use semi_avid_pc::{commit, field, prove, setup::trim, Block};
use tracing::{debug, info, warn};

type UniPoly12_381 = DensePolynomial<<Bls12_381 as Pairing>::ScalarField>;

fn setup<E, P>(nb_bytes: usize) -> Result<Powers<'static, E>, ark_poly_commit::Error>
where
    E: Pairing,
    P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
    for<'a, 'b> &'a P: Div<&'b P, Output = P>,
{
    let degree = nb_bytes / (E::ScalarField::MODULUS_BIT_SIZE as usize / 8);

    let rng = &mut test_rng();

    let params = KZG10::<E, P>::setup(degree, false, rng)?;
    let (powers, _) = trim(params, degree)?;

    Ok(powers)
}

fn run<E, P>(
    bytes: &[u8],
    k: usize,
    n: usize,
    powers: Powers<E>,
) -> Result<Vec<Block<E>>, ark_poly_commit::Error>
where
    E: Pairing,
    P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
    for<'a, 'b> &'a P: Div<&'b P, Output = P>,
{
    info!("encoding and proving {} bytes", bytes.len());

    debug!("splitting bytes into polynomials");
    let elements = field::split_data_into_field_elements::<E>(bytes, k);
    let nb_polynomials = elements.len() / k;
    let polynomials = field::build_interleaved_polynomials::<E, P>(&elements, nb_polynomials);
    info!("data is composed of {} polynomials", polynomials.len());

    debug!("transposing the polynomials to commit");
    let polynomials_to_commit = (0..polynomials[0].coeffs().len())
        .map(|i| P::from_coefficients_vec(polynomials.iter().map(|p| p.coeffs()[i]).collect()))
        .collect::<Vec<P>>();

    debug!("committing the polynomials");
    let (commits, _) = commit(&powers, &polynomials_to_commit)?;

    debug!("creating the {} evaluation points", n);
    let points: Vec<E::ScalarField> = (0..n)
        .map(|i| E::ScalarField::from_le_bytes_mod_order(&[i as u8]))
        .collect();

    debug!("hashing the {} bytes with SHA-256", bytes.len());
    let hash = Sha256::hash(bytes);

    debug!(
        "proving the {} bytes and the {} polynomials",
        bytes.len(),
        polynomials.len()
    );
    prove::<E, P>(commits, hash, bytes.len(), polynomials, &points)
}

fn main() {
    tracing_subscriber::fmt::try_init().expect("cannot init logger");

    let bytes = std::env::args().nth(1).unwrap().as_bytes().to_vec();
    let k: usize = std::env::args().nth(2).unwrap().parse().unwrap();
    let n: usize = std::env::args().nth(3).unwrap().parse().unwrap();
    let generate_powers: bool = std::env::args().nth(4).unwrap().parse().unwrap();
    let powers_file = std::env::args().nth(5).unwrap();

    const COMPRESS: Compress = Compress::Yes;
    const VALIDATE: Validate = Validate::Yes;
    const BLOCK_DIR: &str = "blocks/";

    if generate_powers {
        info!("generating new powers");
        let powers = setup::<Bls12_381, UniPoly12_381>(bytes.len()).unwrap();

        info!("serializing powers");
        let mut serialized = vec![0; powers.serialized_size(COMPRESS)];
        powers
            .serialize_with_mode(&mut serialized[..], COMPRESS)
            .unwrap();

        info!("dumping powers into `{}`", powers_file);
        let mut file = File::create(&powers_file).unwrap();
        file.write_all(&serialized).unwrap();

        exit(0);
    }

    info!("reading powers from file `{}`", powers_file);
    let powers = if let Ok(serialized) = std::fs::read(&powers_file) {
        info!("deserializing the powers from `{}`", powers_file);
        Powers::<Bls12_381>::deserialize_with_mode(&serialized[..], COMPRESS, VALIDATE).unwrap()
    } else {
        warn!("could not read powers from `{}`", powers_file);
        info!("regenerating temporary powers");
        setup::<Bls12_381, UniPoly12_381>(bytes.len()).unwrap()
    };

    let blocks = run::<Bls12_381, UniPoly12_381>(&bytes, k, n, powers).unwrap();

    info!("dumping blocks to `{}`", BLOCK_DIR);
    for (i, block) in blocks.iter().enumerate() {
        let filename = PathBuf::from(BLOCK_DIR).join(format!("{}.bin", i));
        std::fs::create_dir_all(BLOCK_DIR).unwrap();

        debug!("serializing block {}", i);
        let mut serialized = vec![0; block.serialized_size(COMPRESS)];
        block
            .serialize_with_mode(&mut serialized[..], COMPRESS)
            .unwrap();

        debug!("dumping serialized block to `{:?}`", filename);
        let mut file = File::create(&filename).unwrap();
        file.write_all(&serialized).unwrap();
    }
}
