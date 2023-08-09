use image::{Rgba, RgbaImage};
use reqwest::blocking::Client;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::env;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tungstenite::accept;
use tungstenite::Message;
use win_screenshot::prelude::*;
use winapi::shared::windef::HWND;
use winapi::um::winuser::FindWindowA;
use std::io::Cursor;





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
//end broadcasting 

fn parse_params(url: &str) -> std::collections::HashMap<String, String> {
    let mut params = std::collections::HashMap::new();
    let params_start = url.find("?");

    if let Some(start_idx) = params_start {
        let params_string = &url[start_idx + 1..];
        for param in params_string.split("&") {
            let parts: Vec<&str> = param.split("=").collect();
            if parts.len() == 2 {
                params.insert(parts[0].to_string(), parts[1].to_string());
            }
        }
    }

    params
}

fn authenticate(cookie: &str, userid: u64, gameid: u64) -> bool {
    cookie == "admin" && userid == 123 && gameid == 123
}


fn set_broadcaster(broadcaster: Box<Broadcaster>) {
    unsafe {
        BROADCASTER = Some(broadcaster);
    }
}
fn capture_window_screenshot(hwnd: HWND) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let hwnd_isize = hwnd as isize;
    let using = Using::BitBlt;
    let area = Area::ClientOnly;
    let crop_xy = None;
    let crop_wh = None;
    let buf = capture_window_ex(hwnd_isize, using, area, crop_xy, crop_wh)?;

    let image = RgbaImage::from_raw(buf.width, buf.height, buf.pixels)
        .ok_or("Failed to create RgbaImage from captured buffer")?;

    let mut image_data = Cursor::new(Vec::new());
    image.write_to(&mut image_data, image::ImageFormat::Jpeg)?;

    Ok(image_data.into_inner())
}
fn log_error_to_file(err_msg: &str) {
    if let Err(err) = std::fs::write("error.log", err_msg) {
        eprintln!("Error writing to error log: {:?}", err);
    }
}

fn hide_console_window() {
    unsafe { winapi::um::wincon::FreeConsole() };
}

fn main() {
    hide_console_window();
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        let params = parse_params(&args[1]);
        if let (Some(cookie), Some(userid), Some(gameid)) = (
            params.get("cookie"),
            params.get("userid").and_then(|s| s.parse::<u64>().ok()),
            params.get("gameid").and_then(|s| s.parse::<u64>().ok()),    //this is for custom protocals and I have it return the image along with json data to all the websockets!!!
        ) {
            if authenticate(cookie, userid, gameid) {
                println!("Login successful!");
                let window_title = "TARGET WINDOW";
                let broadcaster = create_broadcaster();
                set_broadcaster(broadcaster);
                loop {
                    let window_title_ansi = std::ffi::CString::new(window_title)
                        .expect("CString::new failed");
                    let hwnd = unsafe { FindWindowA(std::ptr::null_mut(), window_title_ansi.as_ptr()) };
                    if hwnd.is_null() {
                        println!("Window not found.");
                        break;
                    }

                    let img_result = capture_window_screenshot(hwnd);

                    match img_result {
                        Ok(img_data) => {
                            let game_data = json!({
                                "gamid": "101",
                                "gameimage": base64::encode(&img_data),
                                "player": "1"
                            });
                            let json_data = serde_json::to_string(&game_data)
                                .unwrap_or_else(|json_err| {
                                    let err_msg = format!("Error while converting to JSON: {:?}", json_err);
                                    println!("{}", err_msg);
                                    log_error_to_file(&err_msg);
                                    String::new()
                                });

                            if let Some(broadcaster) = unsafe { BROADCASTER.as_ref() } {
                                broadcaster(json_data);
                            }
                        }
                        Err(err) => {
                            let err_msg = format!("Error capturing window screenshot: {:?}", err);
                            println!("{}", err_msg);
                            log_error_to_file(&err_msg);
                        }
                    }
                }
            } else {
                println!("Login failed");
            }
        } else {
            println!("Missing parameter.");
        }
    } else {
        println!("No parameters provided.");
    }
}
