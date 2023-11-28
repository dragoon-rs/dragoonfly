use std::io::prelude::*;
use std::ops::Div;
use std::process::exit;
use std::{fs::File, path::PathBuf};

use ark_bls12_381::Bls12_381;
use ark_ec::pairing::Pairing;
use ark_poly::univariate::DensePolynomial;
use ark_poly::DenseUVPolynomial;
use ark_poly_commit::kzg10::Powers;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Compress, Validate};
use tracing::{debug, error, info, warn};

use semi_avid_pc::verify;
use semi_avid_pc::{encode, setup, Block};

type UniPoly12_381 = DensePolynomial<<Bls12_381 as Pairing>::ScalarField>;

const COMPRESS: Compress = Compress::Yes;
const VALIDATE: Validate = Validate::Yes;
const BLOCK_DIR: &str = "blocks/";

fn parse_args() -> (Vec<u8>, usize, usize, bool, String, bool, Vec<String>) {
    let bytes = std::env::args()
        .nth(1)
        .expect("expected bytes as first positional argument")
        .as_bytes()
        .to_vec();
    let k: usize = std::env::args()
        .nth(2)
        .expect("expected k as second positional argument")
        .parse()
        .expect("could not parse k as an int");
    let n: usize = std::env::args()
        .nth(3)
        .expect("expected n as third positional argument")
        .parse()
        .expect("could not parse n as an int");
    let do_generate_powers: bool = std::env::args()
        .nth(4)
        .expect("expected do_generate_powers as fourth positional argument")
        .parse()
        .expect("could not parse do_generate_powers as a bool");
    let powers_file = std::env::args()
        .nth(5)
        .expect("expected powers_file as fifth positional argument");
    let do_verify_blocks: bool = std::env::args()
        .nth(6)
        .expect("expected do_verify_blocks as sixth positional argument")
        .parse()
        .expect("could not parse do_verify_blocks as a bool");
    let block_files = std::env::args().skip(7).collect::<Vec<_>>();

    (
        bytes,
        k,
        n,
        do_generate_powers,
        powers_file,
        do_verify_blocks,
        block_files,
    )
}

fn generate_powers(bytes: &[u8], powers_file: &str) -> Result<(), std::io::Error> {
    info!("generating new powers");
    let powers = setup::random::<Bls12_381, UniPoly12_381>(bytes.len()).unwrap();

    info!("serializing powers");
    let mut serialized = vec![0; powers.serialized_size(COMPRESS)];
    powers
        .serialize_with_mode(&mut serialized[..], COMPRESS)
        .unwrap();

    info!("dumping powers into `{}`", powers_file);
    let mut file = File::create(&powers_file)?;
    file.write_all(&serialized)?;

    Ok(())
}

fn verify_blocks<E, P>(
    block_files: &[String],
    powers: Powers<E>,
) -> Result<(), ark_serialize::SerializationError>
where
    E: Pairing,
    P: DenseUVPolynomial<E::ScalarField, Point = E::ScalarField>,
    for<'a, 'b> &'a P: Div<&'b P, Output = P>,
{
    let mut res = vec![];

    for block_file in block_files {
        if let Ok(serialized) = std::fs::read(&block_file) {
            debug!("deserializing block from `{}`", block_file);
            let block = Block::<E>::deserialize_with_mode(&serialized[..], COMPRESS, VALIDATE)?;
            if verify::<E, P>(&block, &powers) {
                info!("block `{:?} is valid`", block_file);
                res.push(0);
            } else {
                error!("block `{:?} is not valid`", block_file);
                res.push(1);
            }
        } else {
            warn!("could not read from `{:?}`", block_file);
            res.push(2);
        }
    }

    eprint!("[");
    for (block, status) in block_files.iter().zip(res.iter()) {
        eprint!("{{block: {:?}, status: {}}}",block, status);
    }
    eprint!("]");

    Ok(())
}

fn dump_blocks<E: Pairing>(blocks: &[Block<E>]) -> Result<(), std::io::Error> {
    info!("dumping blocks to `{}`", BLOCK_DIR);
    let mut block_files = vec![];
    for (i, block) in blocks.iter().enumerate() {
        let filename = PathBuf::from(BLOCK_DIR).join(format!("{}.bin", i));
        std::fs::create_dir_all(BLOCK_DIR)?;

        debug!("serializing block {}", i);
        let mut serialized = vec![0; block.serialized_size(COMPRESS)];
        block
            .serialize_with_mode(&mut serialized[..], COMPRESS)
            .unwrap();

        debug!("dumping serialized block to `{:?}`", filename);
        let mut file = File::create(&filename)?;
        file.write_all(&serialized)?;

        block_files.push(filename);
    }

    eprint!("[");
    for block_file in &block_files {
        eprint!("{:?},", block_file);
    }
    eprint!("]");

    Ok(())
}

fn main() {
    tracing_subscriber::fmt::try_init().expect("cannot init logger");

    let (bytes, k, n, do_generate_powers, powers_file, do_verify_blocks, block_files) =
        parse_args();

    if do_generate_powers {
        generate_powers(&bytes, &powers_file).unwrap();
        exit(0);
    }

    info!("reading powers from file `{}`", powers_file);
    let powers = if let Ok(serialized) = std::fs::read(&powers_file) {
        info!("deserializing the powers from `{}`", powers_file);
        Powers::<Bls12_381>::deserialize_with_mode(&serialized[..], COMPRESS, VALIDATE).unwrap()
    } else {
        warn!("could not read powers from `{}`", powers_file);
        info!("regenerating temporary powers");
        setup::random::<Bls12_381, UniPoly12_381>(bytes.len()).unwrap()
    };

    if do_verify_blocks {
        verify_blocks::<Bls12_381, UniPoly12_381>(&block_files, powers).unwrap();
        exit(0);
    }

    dump_blocks(&encode::<Bls12_381, UniPoly12_381>(&bytes, k, n, powers).unwrap()).unwrap();
}
