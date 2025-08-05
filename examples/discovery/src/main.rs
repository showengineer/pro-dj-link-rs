use tokio::sync::mpsc;
use clap::Parser;
use pro_dj_link_rs::discovery;
use pro_dj_link_rs::common::CDJDevice;

#[derive(Parser)]
struct Opts {
    /// Interface naam of IPv4 om aan te binden (bv. "Ethernet" of "192.168.1.100").
    #[arg(long)]
    interface: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    let (tx, mut rx) = mpsc::channel::<CDJDevice>(16);
    tokio::spawn(async move {
        if let Err(e) = discovery::listen_for_devices(tx, opts.interface).await {
            eprintln!("Discovery error: {e}");
        }
    });
    while let Some(dev) = rx.recv().await {
        println!("Gevonden device: {:?}", dev);
    }
    Ok(())
}