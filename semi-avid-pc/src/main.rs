use std::io::prelude::*;
use std::process::exit;
use std::{fs::File, path::PathBuf};

use ark_bls12_381::Bls12_381;
use ark_ec::pairing::Pairing;
use ark_poly::univariate::DensePolynomial;
use ark_poly_commit::kzg10::Powers;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Compress, Validate};
use tracing::{debug, error, info, warn};

use semi_avid_pc::verify;
use semi_avid_pc::{encode, setup, Block};

type UniPoly12_381 = DensePolynomial<<Bls12_381 as Pairing>::ScalarField>;

fn main() {
    tracing_subscriber::fmt::try_init().expect("cannot init logger");

    let bytes = std::env::args().nth(1).unwrap().as_bytes().to_vec();
    let k: usize = std::env::args().nth(2).unwrap().parse().unwrap();
    let n: usize = std::env::args().nth(3).unwrap().parse().unwrap();
    let generate_powers: bool = std::env::args().nth(4).unwrap().parse().unwrap();
    let powers_file = std::env::args().nth(5).unwrap();
    let verify_blocks: bool = std::env::args().nth(6).unwrap().parse().unwrap();

    const COMPRESS: Compress = Compress::Yes;
    const VALIDATE: Validate = Validate::Yes;
    const BLOCK_DIR: &str = "blocks/";

    if generate_powers {
        info!("generating new powers");
        let powers = setup::random::<Bls12_381, UniPoly12_381>(bytes.len()).unwrap();

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
        setup::random::<Bls12_381, UniPoly12_381>(bytes.len()).unwrap()
    };

    if verify_blocks {
        for block in std::env::args().skip(7) {
            let block_file = PathBuf::from(BLOCK_DIR).join(block);
            if let Ok(serialized) = std::fs::read(&block_file) {
                debug!("deserializing block from `{:?}`", block_file);
                let block =
                    Block::<Bls12_381>::deserialize_with_mode(&serialized[..], COMPRESS, VALIDATE)
                        .unwrap();
                if verify::<Bls12_381, UniPoly12_381>(&block, &powers) {
                    info!("block `{:?} is valid`", block_file);
                } else {
                    error!("block `{:?} is not valid`", block_file);
                }
            } else {
                warn!("could not read from `{:?}`", block_file);
            }
        }

        exit(0);
    }

    let blocks = encode::<Bls12_381, UniPoly12_381>(&bytes, k, n, powers).unwrap();

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
