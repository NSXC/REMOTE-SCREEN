use image::{Rgba, RgbaImage};
use reqwest::blocking::Client;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tungstenite::accept;
use tungstenite::Message;
use win_screenshot::prelude::*;
use winapi::shared::windef::HWND;
use winapi::um::winuser::FindWindowA;

type PeerMap = Arc<Mutex<HashMap<u64, tungstenite::WebSocket<std::net::TcpStream>>>>;

pub type Broadcaster = dyn Fn(String) + Send;

static mut BROADCASTER: Option<Box<Broadcaster>> = None;

pub fn create_broadcaster() -> Box<Broadcaster> {
    let server = TcpListener::bind("127.0.0.1:8080").expect("Failed to bind to TCP listener");
    let peers: PeerMap = Arc::new(Mutex::new(HashMap::new()));
    
    println!("collecting peers");
    let peers1 = peers.clone();
    thread::spawn(move || {
        for (client_id, stream) in server.incoming().enumerate() {
            let websocket = accept(stream.expect("Error while accepting TCP stream"))
                .expect("Error while accepting WebSocket");
            println!("Registered client {}", client_id);
            peers1.lock().unwrap().insert(client_id as u64, websocket);
        }
    });

    println!("creating closure");
    return Box::new(move |msg| {
        let mut disconnected_clients = vec![];
        for (client_id, sock) in peers.lock().unwrap().iter_mut() {
            let msg1 = msg.clone();
            let out = Message::from(msg1);
            if let Err(err) = sock.write_message(out) {
                eprintln!("Error while writing to WebSocket: {:?}", err);
                disconnected_clients.push(*client_id);
            }
        }
        let mut peers = peers.lock().unwrap();
        for client_id in disconnected_clients {
            peers.remove(&client_id);
        }
    });
}

fn set_broadcaster(broadcaster: Box<Broadcaster>) {
    unsafe {
        BROADCASTER = Some(broadcaster);
    }
}

fn capture_window_screenshot(hwnd: HWND) -> Result<RgbaImage, Box<dyn std::error::Error>> {
    let hwnd_isize = hwnd as isize;
    let using = Using::BitBlt;
    let area = Area::ClientOnly;
    let crop_xy = None;
    let crop_wh = None;
    let buf = capture_window_ex(hwnd_isize, using, area, crop_xy, crop_wh)?;

    let image = RgbaImage::from_raw(buf.width, buf.height, buf.pixels)
        .ok_or("Failed to create RgbaImage from captured buffer")?;

    Ok(image)
}

fn main() {
    let window_title = "WINDOWNAME";
    let broadcaster = create_broadcaster();
    set_broadcaster(broadcaster);
    
    loop {
        let window_title_ansi = std::ffi::CString::new(window_title).expect("CString::new failed");
        let hwnd = unsafe { FindWindowA(std::ptr::null_mut(), window_title_ansi.as_ptr()) };
        if hwnd.is_null() {
            println!("Window not found.");
            break; 
        }

        let img_result = capture_window_screenshot(hwnd);

        match img_result {
            Ok(img) => {
                img.save("screenshot.jpg").expect("Error while saving screenshot");
                let frame = fs::read("screenshot.jpg").expect("Error while reading screenshot file");
                let frame_base64 = base64::encode(&frame);
                let game_data = json!({
                    "gamid": "101",
                    "gameimage": frame_base64,
                    "player": "1"
                });
                let json_data = serde_json::to_string(&game_data).expect("Error while converting to JSON");

                if let Some(broadcaster) = unsafe { BROADCASTER.as_ref() } {
                    broadcaster(json_data);
                }
            }
            Err(err) => {
                println!("Error capturing window screenshot: {:?}", err);
            }
        }

    }
}
