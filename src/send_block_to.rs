mod protocol;

use std::fs as sfs;
use std::io::{BufRead, Write};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use anyhow::Result;
use ark_ec::CurveGroup;
use ark_ff::PrimeField;
use ark_poly::DenseUVPolynomial;
use ark_std::ops::Div;
use chrono::Utc;
use futures::StreamExt;
use libp2p_stream::IncomingStreams;
use tokio::sync::{
    mpsc::{self, Receiver},
    Semaphore,
};
use tracing::{debug, error};

use crate::dragoon_swarm;

pub(crate) use protocol::handle_send_block_exchange_sender_side as send_block_to;

#[derive(Clone)]
pub(crate) struct SendBlockHandler {}

/// An async handler to spawn on a node when we want to automatically manage receiving blocks coming from send requests
impl SendBlockHandler {
    pub(crate) fn run<F, G, P>(
        mut incoming_streams: IncomingStreams,
        powers_path: PathBuf,
        file_dir: PathBuf,
        current_available_storage: Arc<AtomicUsize>,
        total_block_size_on_disk: Arc<AtomicUsize>,
    ) -> Result<()>
    where
        F: PrimeField,
        G: CurveGroup<ScalarField = F>,
        P: DenseUVPolynomial<F>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        tokio::spawn(async move {
            //allow at most 10 send request to be managed at once
            let max_send_request = 10;
            let semaphore = Arc::new(Semaphore::new(max_send_request));
            let (write_to_file_sender, write_to_file_recv) = mpsc::channel(max_send_request);
            tokio::task::spawn_blocking(move || {
                Self::add_new_block_info_to_send_file(write_to_file_recv, total_block_size_on_disk)
            });
            loop {
                let permit = semaphore.clone().acquire_owned().await.unwrap();
                if let Some((peer, stream)) = incoming_streams.next().await {
                    let p_path = powers_path.clone();
                    let f_dir = file_dir.clone();
                    let new_current_available_storage = current_available_storage.clone();
                    let new_write_to_file_sender = write_to_file_sender.clone();
                    tokio::spawn(async move {
                        match protocol::handle_send_block_exchange_recv_side::<F, G, P>(stream, p_path, f_dir, new_current_available_storage, new_write_to_file_sender).await {
                            Ok(_) => {debug!("Finished getting block from peer {} without issue", peer)},
                            Err(e) => error!("The stream with the peer {} for receiving a block due to a send request has been dropped due to an handling error: {}", peer, e)
                        }
                        drop(permit);
                    });
                } else {
                    debug!("We are done with the streams for the send");
                    return Ok::<(), anyhow::Error>(());
                }
            }
        });
        Ok(())
    }

    /// Used to synchronously modify the file that lists all the blocks
    fn add_new_block_info_to_send_file(
        mut receiver: Receiver<(PathBuf, usize, String, String, String)>,
        total_block_size_on_disk: Arc<AtomicUsize>,
    ) {
        while let Some((file_dir, size_of_block, file_hash, block_hash, peer_id_base_58)) =
            receiver.blocking_recv()
        {
            match Self::add_send_file_inner(
                file_dir,
                total_block_size_on_disk.clone(),
                size_of_block,
                file_hash,
                block_hash,
                peer_id_base_58,
            ) {
                Ok(_) => {}
                Err(e) => error!("{}", e),
            }
        }
    }
    fn add_send_file_inner(
        file_dir: PathBuf,
        total_block_size_on_disk: Arc<AtomicUsize>,
        size_of_block: usize,
        file_hash: String,
        block_hash: String,
        peer_id_base_58: String,
    ) -> Result<()> {
        total_block_size_on_disk.fetch_add(size_of_block, Ordering::SeqCst);
        let old_send_file_path: PathBuf =
            [file_dir, PathBuf::from(dragoon_swarm::SEND_BLOCK_FILE_NAME)]
                .iter()
                .collect();
        let mut new_send_file_path = old_send_file_path.clone();
        new_send_file_path.set_extension("new.txt");
        //TODO remove the created file if we return on an error
        let mut new_send_file = sfs::File::options()
            .read(true)
            .append(true)
            .create_new(true)
            .open(&new_send_file_path)?;
        new_send_file.write_all(
            format!(
                "Total: {}\n",
                total_block_size_on_disk.load(Ordering::Relaxed)
            )
            .as_bytes(),
        )?;
        let old_file = sfs::File::open(&old_send_file_path)?;
        let mut old_file = std::io::BufReader::new(old_file);
        // skip the first line (which is the old total)
        old_file.read_line(&mut String::new())?;
        //file is in append mode so we are putting the content of the old file in the new file (except the first line)
        std::io::copy(&mut old_file, &mut new_send_file)?;
        // now append the information about the new block
        new_send_file.write_all(
            format!(
                "Size: {} | Timestamp: {} | file_hash: {} | block_hash: {} | peer_id: {}\n",
                size_of_block,
                Utc::now(),
                file_hash,
                block_hash,
                peer_id_base_58,
            )
            .as_bytes(),
        )?;
        // move the new file on the name of the old file
        sfs::rename(new_send_file_path, old_send_file_path)?;
        Ok(())
    }
}
