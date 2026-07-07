use std::{
    env,
    io::{Read, Write},
    net::TcpStream,
};

use RueSync::requests::Request;
use serde_json;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Usage: client <ip:port> <command>");
        return;
    }

    let addr = &args[1];
    let command = &args[2..].join(" ");

    let request = Request::Basic {
        command: command.to_string(),
    };

    let json = match serde_json::to_string(&request) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to serialize request: {}", e);
            return;
        }
    };

    match TcpStream::connect(addr) {
        Ok(mut stream) => {
            if let Err(e) = stream.write_all(json.as_bytes()) {
                eprintln!("Write error: {}", e);
                return;
            }

            let mut buffer = [0u8; 1024];

            match stream.read(&mut buffer) {
                Ok(size) => {
                    let response = String::from_utf8_lossy(&buffer[..size]);
                    println!("{}", response);
                }

                Err(e) => eprintln!("Read error: {}", e),
            }
        }

        Err(e) => eprintln!("Connection failed: {}", e),
    }
}
