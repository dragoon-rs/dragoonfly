use anyhow::{format_err, Result};
use ark_ec::CurveGroup;
use ark_ff::PrimeField;
use ark_poly::DenseUVPolynomial;
use ark_serialize::{CanonicalDeserialize, Compress, Validate};
use ark_std::ops::Div;
use futures::{AsyncReadExt, AsyncWriteExt};
use komodo::{verify, Block};
use libp2p::{PeerId, Stream};
use std::path::PathBuf;
use std::{
    mem::size_of,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use strum::FromRepr;
use tokio::fs::{self, File};
use tokio::sync::mpsc::Sender;

use tracing::{debug, error, info, warn};

use komodo::zk::Powers;

use crate::send_strategy::SendId;
use crate::{
    dragoon_swarm::{get_block_dir, get_powers},
    peer_block_info::PeerBlockInfo,
};

const MAX_PBI_SIZE: usize = 1024; // max size in bytes for a peer block info

#[derive(Debug, Clone, Copy, FromRepr)]
#[repr(u8)]
enum ExchangeCode {
    AcceptBlockSend,
    RejectBlockSend,
    BlockIsCorrect,
    BlockIsIncorrect,
}

// -------------------- SENDER -------------------- //

/// Build the information regarding the block to be sent, includes the block hash, file hash, and size of the block
async fn build_peer_block_info(
    peer_id: PeerId,
    block_hash: String,
    file_hash: String,
    file_dir: PathBuf,
) -> Result<PeerBlockInfo> {
    let block_dir = get_block_dir(&file_dir, file_hash.clone());
    let block_path: PathBuf = [block_dir, PathBuf::from(block_hash.clone())]
        .iter()
        .collect();
    let block_file = File::open(block_path).await?;
    let block_size = block_file.metadata().await?.len();

    Ok(PeerBlockInfo {
        peer_id_base_58: peer_id.to_base58(),
        file_hash,
        block_hashes: vec![block_hash],
        block_sizes: Some(vec![block_size as usize]),
    })
}

/// Send the peer block info to the other end of the stream
async fn send_peer_block_info(
    stream: &mut Stream,
    own_peer_id: PeerId,
    block_hash: String,
    file_hash: String,
    file_dir: PathBuf,
) -> Result<()> {
    let peer_block_info =
        build_peer_block_info(own_peer_id, block_hash, file_hash, file_dir).await?;
    let ser_peer_block_info = serde_json::to_vec(&peer_block_info)?;
    let size_of_pbi = ser_peer_block_info.len();
    stream.write_all(&usize::to_be_bytes(size_of_pbi)).await?;
    stream.write_all(&ser_peer_block_info).await?;
    Ok(())
}

/// Send the block to the other end of the stream
async fn send_block(
    stream: &mut Stream,
    block_hash: String,
    file_hash: String,
    file_dir: PathBuf,
) -> Result<()> {
    let block_dir = get_block_dir(&file_dir, file_hash.clone());
    let block_path: PathBuf = [block_dir, PathBuf::from(block_hash.clone())]
        .iter()
        .collect();
    let ser_block = fs::read(block_path).await?;
    stream.write_all(&ser_block).await?;

    Ok(())
}

/// Main function for the sender side, will attempt to send the block, can fail if the other end refuses to get the block.
/// This is a oneshot try, meaning there is no logic behind to try to find another peer to get the block.
pub(crate) async fn handle_send_block_exchange_sender_side(
    stream: Stream, //TODO give a &mut stream instead so the caller can close the stream on all errors
    own_peer_id: PeerId,
    recv_peer_id: PeerId,
    block_hash: String,
    file_hash: String,
    file_dir: PathBuf,
) -> Result<(bool, SendId), SendId> {
    handle_send_block_exchange_sender_side_inner(
        stream,
        own_peer_id,
        recv_peer_id,
        block_hash.clone(),
        file_hash.clone(),
        file_dir,
    )
    .await
    .map_err(|_| SendId {
        peer_id: recv_peer_id,
        file_hash,
        block_hash,
    })
}

async fn handle_send_block_exchange_sender_side_inner(
    mut stream: Stream, //TODO give a &mut stream instead so the caller can close the stream on all errors
    own_peer_id: PeerId,
    recv_peer_id: PeerId,
    block_hash: String,
    file_hash: String,
    file_dir: PathBuf,
) -> Result<(bool, SendId)> {
    send_peer_block_info(
        &mut stream,
        own_peer_id,
        block_hash.clone(),
        file_hash.clone(),
        file_dir.clone(),
    )
    .await?;
    let mut ser_answer = [0u8; 1];
    stream.read_exact(&mut ser_answer).await?;
    let send_id = SendId {
        peer_id: recv_peer_id,
        file_hash: file_hash.clone(),
        block_hash: block_hash.clone(),
    };
    if let Some(answer) = ExchangeCode::from_repr(ser_answer[0]) {
        match answer {
            ExchangeCode::AcceptBlockSend => {}
            ExchangeCode::RejectBlockSend => {
                stream.close().await?;
                return Ok((false, send_id));
            }
            a => {
                let err_string = format!("Unexpected ExchangeCode variant for answer {:?}", a);
                warn!(err_string);
                stream.close().await?;
                return Err(format_err!(err_string));
            }
        }
    } else {
        let err_string = format!(
            "Unknown ExchangeCode variant discriminant for answer {}",
            ser_answer[0]
        );
        warn!(err_string);
        stream.close().await?;
        return Err(format_err!(err_string));
    }

    // block got accepted, we send it
    send_block(&mut stream, block_hash, file_hash, file_dir).await?;
    let mut ser_block_status = [0u8; 1];
    stream.read_exact(&mut ser_block_status).await?;
    stream.close().await?;
    debug!("ser block status: {:?}", ser_block_status);
    if let Some(block_status) = ExchangeCode::from_repr(ser_block_status[0]) {
        match block_status {
            ExchangeCode::BlockIsCorrect => Ok((true, send_id)),
            ExchangeCode::BlockIsIncorrect => Ok((false, send_id)),
            a => {
                let err_string = format!("Unexpected ExchangeCode variant for block status{:?}", a);
                warn!(err_string);
                stream.close().await?;
                Err(format_err!(err_string))
            }
        }
    } else {
        let err_string = format!(
            "Unknown ExchangeCode variant discriminant for block status {}",
            ser_answer[0]
        );
        warn!(err_string);
        stream.close().await?;
        Err(format_err!(err_string))
    }
}

// -------------------- RECEIVER -------------------- //

/// Choose whether or not to accept the send request.
/// Remove from the total available storage when choosing to accept the block, returning the choice to accept or reject the block and the size by which the total storage space was changed.
/// Returning the change of storage space allows to revert the change later on if we end up rejecting the block for other reasons.
async fn choose_response_to_send_request(
    peer_block_info: &PeerBlockInfo,
    current_available_storage: Arc<AtomicUsize>,
) -> (ExchangeCode, usize) {
    if let Some(block_size_vec) = peer_block_info.block_sizes.as_ref() {
        if let Some(size) = block_size_vec.first() {
            let available_storage = current_available_storage.load(Ordering::Relaxed);
            if &available_storage > size {
                // send the new available storage space since we decided to accept the block
                current_available_storage.store(available_storage - size, Ordering::Relaxed);
                info!("New available storage space: {}", available_storage - size);
                (ExchangeCode::AcceptBlockSend, *size)
            } else {
                (ExchangeCode::RejectBlockSend, 0)
            }
        } else {
            warn!("No size was provided for the block to be received by a send request");
            (ExchangeCode::RejectBlockSend, 0)
        }
    } else {
        warn!("No size was provided for the block to be received by a send request");
        (ExchangeCode::RejectBlockSend, 0)
    }
}

/// Send back the response to the send request
async fn respond_to_send_request(stream: &mut Stream, answer: ExchangeCode) -> Result<()> {
    let ser_answer = [answer as u8];
    stream.write_all(&ser_answer).await?;

    Ok(())
}

/// Send back the block status to tell the sender if the block they sent was valid and stored, or invalid and thus not stored
async fn send_block_status(stream: &mut Stream, block_status: ExchangeCode) -> Result<()> {
    let ser_status = [block_status as u8];
    stream.write_all(&ser_status).await?;

    Ok(())
}

/// Handles receiving the block in itself and deserializing it
async fn receive_block<F, G>(
    stream: &mut Stream,
    peer_block_info: &PeerBlockInfo,
) -> Result<(Vec<u8>, Block<F, G>)>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    let PeerBlockInfo { block_sizes, .. } = peer_block_info;
    if let Some(vec_size) = block_sizes {
        if let Some(size) = vec_size.first() {
            let mut ser_block = vec![0u8; *size];
            stream.read_exact(&mut ser_block[..]).await?;
            let block = Block::deserialize_with_mode(&ser_block[..], Compress::Yes, Validate::Yes)?;
            Ok((ser_block, block))
        } else {
            Err(format_err!("A size vector was provided to read the block that was sent, but the vector was empty"))
        }
    } else {
        Err(format_err!(
            "No size vector was provided to read the block that was sent"
        ))
    }
}

/// Handles the entire transaction for the receiver side of the block send
pub(super) async fn handle_send_block_exchange_recv_side<F, G, P>(
    mut stream: Stream,
    powers_path: PathBuf,
    file_dir: PathBuf,
    current_available_storage: Arc<AtomicUsize>,
    write_to_file_sender: Sender<(PathBuf, usize, String, String, String)>,
) -> Result<()>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
    P: DenseUVPolynomial<F>,
    for<'a, 'b> &'a P: Div<&'b P, Output = P>,
{
    // receive the size of the peer block info
    let mut ser_peer_block_info_size = [0u8; size_of::<usize>()];
    stream.read_exact(&mut ser_peer_block_info_size).await?;
    let peer_block_info_size = usize::from_be_bytes(ser_peer_block_info_size);

    if peer_block_info_size > MAX_PBI_SIZE {
        stream.close().await?;
        return Err(format_err!(
            "The peer block info's size of {} was bigger than the maximum peer block size of {}",
            peer_block_info_size,
            MAX_PBI_SIZE,
        ));
    }
    // receive the peer block info
    let mut ser_peer_block_info = vec![0u8; peer_block_info_size];
    stream.read_exact(&mut ser_peer_block_info[..]).await?;
    let peer_block_info: PeerBlockInfo = serde_json::de::from_slice(&ser_peer_block_info)?;
    let (answer, size_change) =
        choose_response_to_send_request(&peer_block_info, current_available_storage.clone()).await;

    match send_block_recv_wrapper::<F, G, P>(
        &mut stream,
        answer,
        powers_path,
        &file_dir,
        peer_block_info,
    )
    .await
    {
        Ok((file_hash, block_hash, peer_id_base_58)) => {
            match write_to_file_sender
                .send((
                    file_dir,
                    size_change,
                    file_hash,
                    block_hash,
                    peer_id_base_58,
                ))
                .await
            {
                Ok(_) => {}
                Err(_) => {
                    stream.close().await.map_err(|e| -> anyhow::Error {format_err!("Got tow errors: couldn't call to write to the list of send block file and {:?}", e)})?;
                    return Err(format_err!(
                        "The call to write to the list of send block file failed"
                    ));
                }
            }
        } //TODO change the available size in the send block file and add information about the block by sending the information through a sender
        Err(e) => {
            current_available_storage.fetch_add(size_change, Ordering::Relaxed);

            stream.close().await?;
            return Err(e);
        }
    }
    Ok(())
}

/// A wrapper after the part where we choose to accept or reject the block.
/// This is used to catch the errors before they are returned and reverting the change to the available storage (so we free the space that we previously said we would use)
async fn send_block_recv_wrapper<F, G, P>(
    stream: &mut Stream,
    answer: ExchangeCode,
    powers_path: PathBuf,
    file_dir: &PathBuf,
    peer_block_info: PeerBlockInfo,
) -> Result<(String, String, String)>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
    P: DenseUVPolynomial<F>,
    for<'a, 'b> &'a P: Div<&'b P, Output = P>,
{
    respond_to_send_request(stream, answer).await?;
    match answer {
        ExchangeCode::AcceptBlockSend => {}
        ExchangeCode::RejectBlockSend => {
            stream.close().await?;
            return Ok(Default::default());
        }
        a => {
            let err_msg = format!(
                "Wrong enum variant provided by `choose_response_to_send_request`: {:?}",
                a
            );
            error!(err_msg);
            return Err(format_err!(err_msg));
        }
    }
    // receive the block
    let (ser_block, block) = receive_block::<F, G>(stream, &peer_block_info).await?;
    let PeerBlockInfo {
        peer_id_base_58,
        file_hash,
        block_hashes,
        ..
    } = peer_block_info;
    let block_hash = if let Some(block_hash) = block_hashes.first() {
        block_hash
    } else {
        let err_msg = format!(
            "No block hash has been provided for the block to be sent by {}",
            peer_id_base_58
        );
        error!(err_msg);
        return Err(format_err!(err_msg));
    };
    // at this point we have the block deserialized, but we don't know if it's correct or not
    let powers: Powers<F, G> = get_powers(powers_path).await?;
    // check that the block is correct
    if verify(&block, &powers)? {
        let block_dir = get_block_dir(file_dir, file_hash.clone());
        tokio::fs::create_dir_all(&block_dir).await?;
        let block_path: PathBuf = [block_dir, PathBuf::from(block_hash.clone())]
            .iter()
            .collect();
        debug!("Will write the received block to {:?}", block_path);
        tokio::fs::write(block_path, ser_block).await?;
        send_block_status(stream, ExchangeCode::BlockIsCorrect).await?;
    } else {
        send_block_status(stream, ExchangeCode::BlockIsIncorrect).await?;
    }
    stream.close().await?;
    Ok((file_hash, block_hash.clone(), peer_id_base_58))
}
