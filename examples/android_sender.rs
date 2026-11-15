//! Exercise the real desktop one-shot sender against an Android receiver.
#[allow(dead_code)]
#[path = "../src/backend.rs"]
mod backend;
use backend::{sendme_send, SendmeSendOptions, TransferMsg};
use std::{path::PathBuf, time::Duration};

fn main() {
    let args:Vec<_>=std::env::args().collect();
    let (events,handle)=sendme_send(SendmeSendOptions{
        paths:vec![PathBuf::from(&args[2])],one_shot:true,blob_dir:Some(PathBuf::from(&args[3])),
    },"");
    let result=loop {
        match events.recv_timeout(Duration::from_secs(150)) {
            Ok(TransferMsg::Code(ticket))=>println!("TICKET={ticket}"),
            Ok(TransferMsg::PeerDisconnected)=>{println!("SENDER_ENDED=completed");break true},
            Ok(TransferMsg::Error(error))=>{eprintln!("SENDER_ENDED=failed: {error}");break false},
            Ok(_)=>{},
            Err(error)=>{eprintln!("Sender ended without success: {error}");break false},
        }
    };
    handle.request_cancel();
    if !result{std::process::exit(1)}
}
