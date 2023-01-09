use anyhow::Result;
use tokio::time::sleep;
//use std::str::FromStr;
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

#[derive(Serialize, Deserialize, Debug)]
struct Application {
    name: String,
    version: String,
    author: String,
    description: String,
    date: String
}

#[derive(Serialize, Deserialize, Debug)]
struct Device {
    name: String
}

impl Device {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string()
        }
    }
    fn get_name(&self) -> &str {
        &self.name
    }
}

impl Application {
    fn get_name(&self) -> &str {
        &self.name
    }
    fn get_version(&self) -> &str {
        &self.version
    }
    fn get_author(&self) -> &str {
        &self.author
    }
    fn get_description(&self) -> &str {
        &self.description
    }
    fn get_date(&self) -> &str {
        &self.date
    }
}


#[tokio::main]
async fn main() -> Result<()> {
    let mut run: bool = true;
    //let mut rerun: bool = false;
    let app: Application = create_application().await;
    let device: Device = create_device("kamera").await;
    intro(&app).await;
    //let listener = UdpSocket::bind("127.0.0.1:5004").await.unwrap();
    let list = Arc::new(UdpSocket::bind("127.0.0.1:5004").await.unwrap());
    loop {
        restart_info(&device).await;
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
                //let d_id = d.id();
                println!("[NEW DATACHANNEL]: {}", d_label);

                Box::pin(async move {
                    let _d2 = Arc::clone(&d);
                    let d_label2 = d_label.clone();
                    //let d_id2 = d_id;
                    d.on_open(Box::new(move || {
                        println!("[DATACHANNEL OPEN]: {}", d_label2);
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
                println!("[ICE CONNECTION]: {}", connection_state);
                if connection_state == RTCIceConnectionState::Disconnected {
                    let _ = done_tx1.try_send(());
                }
                if connection_state == RTCIceConnectionState::Failed {
                    let _ = done_tx1.try_send(());
                }
                Box::pin(async {})
            })).await;

        let done_tx2 = done_tx.clone();

        peer_connection
            .on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
                println!("[PEER CONNECTION]: {}", s);
                if s == RTCPeerConnectionState::Failed {
                    // Wait until PeerConnection has had no network activity for 30 seconds or another failure. It may be reconnected using an ICE Restart.
                    // Use webrtc.PeerConnectionStateDisconnected if you are interested in detecting faster timeout.
                    // Note that the PeerConnection may come back from PeerConnectionStateDisconnected.
                    let _ = done_tx2.try_send(());
                }
                Box::pin(async {})
            })).await;
        
        
        let offer_encoded = wait_offer(device.get_name()).await;

        let desc_data = signal::decode(&offer_encoded)?;
        let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
        peer_connection.set_remote_description(offer).await?;
        let answer = peer_connection.create_answer(None).await?;

        let mut gather_complete = peer_connection.gathering_complete_promise().await;
        peer_connection.set_local_description(answer).await?;
        let _ = gather_complete.recv().await;

        if let Some(local_desc) = peer_connection.local_description().await {
            send_answer(&local_desc, device.get_name()).await;
        } else {
            println!("[ANSWER]: failed!");
        }
        let done_tx3 = done_tx.clone();
        
        let l = Arc::clone(&list);
        tokio::spawn(async move {
            let mut inbound_rtp_packet = vec![0u8; 1600]; // UDP MTU
            while let Ok((n, _)) = l.recv_from(&mut inbound_rtp_packet).await {
                if let Err(err) = video_track.write(&inbound_rtp_packet[..n]).await {
                    if Error::ErrClosedPipe == err {
                    } else {
                        println!("[VIDEO ERROR]: {}", err);
                    }
                    let _ = done_tx3.try_send(());
                    return;
                }
            }
        });

        tokio::select! {
            _ = done_rx.recv() => {
                println!("[SIGNAL]: done");
            }
            _ = tokio::signal::ctrl_c() => {
                println!("");
                run = false;
            }
        };
        peer_connection.close().await?;
        if !run {
            break;
        }
        //rerun = true;
        //listener = UdpSocket::bind("127.0.0.1:9004").await.unwrap();
    }
    Ok(())
}

async fn wait(seconds: u32) {
    sleep(Duration::from_secs(seconds as u64)).await;
}

async fn create_device(name: &str) -> Device {
    let device: Device = Device::new(name);
    device
}

async fn create_application() -> Application {
    let app: Application=Application {
        name: "ArtCam".to_owned(),
        version: "0.5.0".to_owned(),
        author: "Artur Gwoździowski".to_owned(),
        description:
        "Service application used to remote controll robots. Implemented in RUST programming language. Main features: 
        - ability to establish p2p connection over public internet, 
        - real-time robot camera video streaming, 
        - sending commands to robot and sending back telemetry data"
        .to_owned(),
        date: "2022-10-10".to_owned()
    };
    app
}

async fn intro(app: &Application) {
    println!("{}", app.get_name());
    wait(1).await;
    println!("version {}", app.get_version());
    wait(1).await;
    println!("author {}", app.get_author());
    wait(1).await;
    println!("date {}", app.get_date());
    wait(1).await;
    println!("{}", app.get_description());
    wait(3).await;
}

async fn restart_info(device: &Device) {
    println!("device {}", device.get_name());
    wait(1).await;
    println!("connecting...");
    wait(3).await;
}

async fn wait_offer(device: &str) -> String {
    let firebase = Firebase::new("https://rtp-to-webrtc-default-rtdb.firebaseio.com")
        .unwrap().at("signaling").at(&device).at("offer");
    let mut offer_founded: bool=false;
    let mut offer_b64: String=String::new();
    println!("waiting for offer...");
    sleep(Duration::from_secs(1)).await;
    
    while !offer_founded {
        let encod = firebase.get::<String>().await;
        match encod  {
            Ok(v) if v != "" => {
                offer_b64 = v;
                offer_founded = true;
                let firebase2 = Firebase::new("https://rtp-to-webrtc-default-rtdb.firebaseio.com")
                    .unwrap().at("signaling").at(&device);
                let clear_offer: Offer=Offer { offer: "".to_string() };
                firebase2.update(&clear_offer).await.unwrap();
            },
            Ok(_) => {
                sleep(Duration::from_secs(10)).await;
            },
            Err(_) => {
                sleep(Duration::from_secs(3)).await;
            }
        }
    }
    println!("OFFER: [OK]");
    offer_b64
}

async fn send_answer(answer: &RTCSessionDescription, device: &str) {
    let json_str = serde_json::to_string(answer).unwrap();
    let b64 = signal::encode(&json_str);
    let firebase = Firebase::new("https://rtp-to-webrtc-default-rtdb.firebaseio.com")
        .unwrap().at("signaling").at(device);
    let ans: Answer=Answer { answer: b64 };
    firebase.update(&ans).await.unwrap();
}