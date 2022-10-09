use anyhow::Result;
use clap::{AppSettings, Arg, Command};
use tokio::time::sleep;
use std::io::Write;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_VP8};
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::{math_rand_alpha};
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::{TrackLocal, TrackLocalWriter};
use webrtc::Error;
use serde::{Serialize, Deserialize};
use firebase_rs::*;


#[derive(Serialize, Deserialize, Debug)]
struct Answer {
  answer: String
}

#[derive(Serialize, Deserialize, Debug)]
struct Offer {
    offer: String
}


#[tokio::main]
async fn main() -> Result<()> {
    let mut app = Command::new("rtp-forwarder")
        .version("0.2.3")
        .author("Artur Gwoździowski")
        .about("An example of rtp-forwarder.")
        .setting(AppSettings::DeriveDisplayOrder)
        .subcommand_negates_reqs(true)
        .arg(
            Arg::new("IDENT")
                .help("some kind of callsign to identifying peer and data-channel name")
                .long("ident")
                .short('i')
                .takes_value(true)
                .default_value(&"kamerka")
        )
        .arg(
            Arg::new("FULLHELP")
                .help("Prints more detailed help information")
                .long("fullhelp"),
        )
        .arg(
            Arg::new("debug")
                .long("debug")
                .short('d')
                .help("Prints debug log information"),
        );

    let matches = app.clone().get_matches();

    if matches.is_present("FULLHELP") {
        app.print_long_help().unwrap();
        std::process::exit(0);
    }
    let version = Command::get_version(&app).unwrap();
    println!("{}", version);
    let debug = matches.is_present("debug");
    if debug {
        env_logger::Builder::new()
            .format(|buf, record| {
                writeln!(
                    buf,
                    "{}:{} [{}] {} - {}",
                    record.file().unwrap_or("unknown"),
                    record.line().unwrap_or(0),
                    record.level(),
                    chrono::Local::now().format("%H:%M:%S.%6f"),
                    record.args()
                )
            })
            .filter(None, log::LevelFilter::Trace)
            .init();
    }

    //let identify = value_t!(app, "IDENT", String).unwrap();
    let identify: String=String::from_str(&"kamera").unwrap();
    println!("device: {}", identify);
    println!("connecting...");
    sleep(Duration::from_secs(1)).await;

    let mut m = MediaEngine::default();

    m.register_default_codecs()?;

    let mut registry = Registry::new();

    registry = register_default_interceptors(registry, &mut m)?;

    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()}],
        ..Default::default()
    };

    let peer_connection = Arc::new(api.new_peer_connection(config).await?);

    let video_track = Arc::new(TrackLocalStaticRTP::new(
        RTCRtpCodecCapability {mime_type: MIME_TYPE_VP8.to_owned(),
            ..Default::default()}, "video".to_owned(), "webrtc-rs".to_owned(), ));

    let rtp_sender = peer_connection
        .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>).await?;

    tokio::spawn(async move {
        let mut rtcp_buf = vec![0u8; 1500];
        while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
        Result::<()>::Ok(())
    });

    let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<()>(1);
    let done_tx1 = done_tx.clone();
    
    let (data_tx, mut data_rx) = tokio::sync::mpsc::channel::<()>(1);
    //let data_tx1 = data_tx.clone();

    peer_connection
        .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
            let d_label = d.label().to_owned();
            let d_id = d.id();
            println!("New DataChannel {} {}", d_label, d_id);

            Box::pin(async move {
                let d2 = Arc::clone(&d);
                let d_label2 = d_label.clone();
                let d_id2 = d_id;
                d.on_open(Box::new(move || {
                    println!("Data channel '{}'-'{}' open. Random messages will now be sent to any connected DataChannels every 5 seconds", d_label2, d_id2);
                    Box::pin(async move {
                        let mut result = Result::<usize>::Ok(0);
                        while result.is_ok() {
                            let timeout = tokio::time::sleep(Duration::from_secs(5));
                            tokio::pin!(timeout);

                            tokio::select! {
                                _ = timeout.as_mut() => {
                                    let message = math_rand_alpha(15);
                                    println!("Sending '{}'", message);
                                    result = d2.send_text(message).await.map_err(Into::into);
                                }
                            };
                        }
                    })
                })).await;

                d.on_message(Box::new(move |msg: DataChannelMessage| {
                    let msg_str = String::from_utf8(msg.data.to_vec()).unwrap();
                    println!("'{}': '{}'", d_label, msg_str);
                    Box::pin(async {})
                })).await;
            })
        }))
        .await;

    peer_connection
        .on_ice_connection_state_change(Box::new(move |connection_state: RTCIceConnectionState| {
            println!("Connection State has changed {}", connection_state);
            if connection_state == RTCIceConnectionState::Failed {
                let _ = done_tx1.try_send(());
            }
            Box::pin(async {})
        })).await;

    let done_tx2 = done_tx.clone();

    peer_connection
        .on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            println!("Peer Connection State has changed: {}", s);
            if s == RTCPeerConnectionState::Failed {
                // Wait until PeerConnection has had no network activity for 30 seconds or another failure. It may be reconnected using an ICE Restart.
                // Use webrtc.PeerConnectionStateDisconnected if you are interested in detecting faster timeout.
                // Note that the PeerConnection may come back from PeerConnectionStateDisconnected.
                println!("Peer Connection has gone to failed exiting: Done forwarding");
                let _ = done_tx2.try_send(());
            }
            Box::pin(async {})
        })).await;
    
    //? [[[ODBIÓR OFERTY]]] ?/
    let firebase = Firebase::new("https://rtp-to-webrtc-default-rtdb.firebaseio.com")
                                .unwrap().at("signaling").at(&identify).at("offer");
    let mut offer_ok: bool=false;
    let mut offer_encoded: String=String::new();
    println!("waiting for offer...");
    sleep(Duration::from_secs(1)).await;
    while !offer_ok {
        let encod = firebase.get::<String>().await;
        match encod  {
            Ok(v) if v != "" => {
                offer_encoded = v;
                offer_ok = true;
                let firebase2 = Firebase::new("https://rtp-to-webrtc-default-rtdb.firebaseio.com")
                                            .unwrap().at("signaling").at(&identify);
                let clear_offer: Offer=Offer { offer: "".to_string() };
                firebase2.update(&clear_offer).await.unwrap();
            },
            Ok(_) => {
                sleep(Duration::from_secs(3)).await;
            },
            Err(_) => {
                sleep(Duration::from_secs(3)).await;
            }
        }
    }

    println!("OFFER: [OK]");
    //let line = signal::must_read_stdin()?;
    let desc_data = signal::decode(&offer_encoded)?;
    let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
    peer_connection.set_remote_description(offer).await?;
    let answer = peer_connection.create_answer(None).await?;

    // Create channel that is blocked until ICE Gathering is complete
    let mut gather_complete = peer_connection.gathering_complete_promise().await;
    // Sets the LocalDescription, and starts our UDP listeners
    peer_connection.set_local_description(answer).await?;
    // Block until ICE Gathering is complete, disabling trickle ICE
    // we do this because we only can exchange one signaling message
    // in a production application you should exchange ICE Candidates via OnICECandidate
    let _ = gather_complete.recv().await;

    // Output the answer in base64 so we can paste it in browser
    if let Some(local_desc) = peer_connection.local_description().await {
        let json_str = serde_json::to_string(&local_desc)?;
        let b64 = signal::encode(&json_str);
        let firebase = Firebase::new("https://rtp-to-webrtc-default-rtdb.firebaseio.com")
                                .unwrap().at("signaling").at(&identify);
        let ans: Answer=Answer { answer: b64 };
        firebase.update(&ans).await.unwrap();
        //println!("{}", b64);
    } else {
        println!("generate local_description failed!");
    }

    // Open a UDP Listener for RTP Packets on port 5004
    let listener = UdpSocket::bind("127.0.0.1:5004").await?;

    let done_tx3 = done_tx.clone();
    // Read RTP packets forever and send them to the WebRTC Client
    tokio::spawn(async move {
        let mut inbound_rtp_packet = vec![0u8; 1600]; // UDP MTU
        while let Ok((n, _)) = listener.recv_from(&mut inbound_rtp_packet).await {
            if let Err(err) = video_track.write(&inbound_rtp_packet[..n]).await {
                if Error::ErrClosedPipe == err {
                    // The peerConnection has been closed.
                } else {
                    println!("video_track write err: {}", err);
                }
                let _ = done_tx3.try_send(());
                return;
            }
        }
    });

    println!("Press ctrl-c to stop");
    tokio::select! {
        _ = done_rx.recv() => {
            println!("received done signal!");
        }
        _ = tokio::signal::ctrl_c() => {
            println!("");
        }
    };

    peer_connection.close().await?;

    Ok(())
}
