use anyhow::Result;
use tokio::time::sleep;
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
use webrtc::ice_transport::ice_credential_type::RTCIceCredentialType;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
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
    let app_name = "ArtCam(simple)";
    let version = "0.3.3";
    let author = "Artur Gwoździowski";
    let desctiption = "RUST implementation of robotic peer application used to remote control device with real-time camera stream through public internet.";
    println!("{}     [v{}]", app_name, version);
    println!("author: {}", author);
    println!("{}", desctiption);
    sleep(Duration::from_secs(6)).await;

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
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:fr-turn1.xirsys.com".to_owned()],
            username: "23Xgr3XVCOk2GqoZW5eWhbdXM1EfA8VcC6OVVacJSpFdoljTUOsTcgAoFUvfN4vcAAAAAGNFN29nd296ZHlrMg==".to_owned(),
            credential: "2ed490ce-4947-11ed-bd3d-0242ac120004".to_owned(),
            credential_type: RTCIceCredentialType::Password.to_owned()
        }],
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
    let (_data_tx, mut _data_rx) = tokio::sync::mpsc::channel::<()>(1);

    peer_connection
        .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
            let d_label = d.label().to_owned();
            let d_id = d.id();
            println!("[NEW DATACHANNEL]: {} {}", d_label, d_id);

            Box::pin(async move {
                let _d2 = Arc::clone(&d);
                let d_label2 = d_label.clone();
                let d_id2 = d_id;
                d.on_open(Box::new(move || {
                    println!("[DATACHANNEL] {}<{}> open", d_label2, d_id2);
                    Box::pin(async move {})
                })).await;

                d.on_message(Box::new(move |msg: DataChannelMessage| {
                    let msg_str = String::from_utf8(msg.data.to_vec()).unwrap();
                    println!("[↓]: {}", msg_str);
                    Box::pin(async {})
                })).await;
            })
        })).await;

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
    let desc_data = signal::decode(&offer_encoded)?;
    let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
    peer_connection.set_remote_description(offer).await?;
    let answer = peer_connection.create_answer(None).await?;

    let mut gather_complete = peer_connection.gathering_complete_promise().await;
    peer_connection.set_local_description(answer).await?;
    let _ = gather_complete.recv().await;

    if let Some(local_desc) = peer_connection.local_description().await {
        let json_str = serde_json::to_string(&local_desc)?;
        let b64 = signal::encode(&json_str);
        let firebase = Firebase::new("https://rtp-to-webrtc-default-rtdb.firebaseio.com")
            .unwrap().at("signaling").at(&identify);
        let ans: Answer=Answer { answer: b64 };
        firebase.update(&ans).await.unwrap();
    } else {
        println!("generate local_description failed!");
    }

    let listener = UdpSocket::bind("127.0.0.1:5004").await?;
    let done_tx3 = done_tx.clone();
    tokio::spawn(async move {
        let mut inbound_rtp_packet = vec![0u8; 1600]; // UDP MTU
        while let Ok((n, _)) = listener.recv_from(&mut inbound_rtp_packet).await {
            if let Err(err) = video_track.write(&inbound_rtp_packet[..n]).await {
                if Error::ErrClosedPipe == err {
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
