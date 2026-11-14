use std::{path::PathBuf, time::Duration, sync::Arc};
struct Events;
impl sendme::EventEmitter for Events {
    fn emit_event(&self, _: &str) -> Result<(), String> { Ok(()) }
    fn emit_event_with_payload(&self, name: &str, value: &str) -> Result<(), String> {
        if name.contains("route") || name.contains("paths") { eprintln!("{name} {value}"); }
        Ok(())
    }
}
fn main() -> anyhow::Result<()> {
    if let Ok(filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).with_ansi(false).init();
    }
    let a: Vec<_> = std::env::args().collect();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        if a[1] == "send" {
            let share = sendme::start_share(
                vec![PathBuf::from(&a[2])],
                "DataBeam_databeam".into(),
                sendme::SendOptions {
                    blob_dir: Some(PathBuf::from(&a[3])),
                    ticket_type: sendme::AddrInfoOptions::RelayAndAddresses,
                    ..Default::default()
                },
                Some(Arc::new(Events)),
            )
            .await?;
            println!("TICKET={}", share.ticket);
            tokio::time::sleep(Duration::from_secs(150)).await;
            share.router.shutdown().await?;
        } else {
            let result = sendme::download(
                a[2].clone(),
                sendme::ReceiveOptions {
                    output_dir: Some(PathBuf::from(&a[3])),
                    blob_dir: Some(PathBuf::from(&a[4])),
                    ..Default::default()
                },
                Some(Arc::new(Events)),
            )
            .await?;
            println!("RECEIVED={}", result.file_path.display());
        }
        anyhow::Ok(())
    })
}
