use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use arch_sdk::Config;
use serde_json::{Value, json};

// A finite response script makes extra RPC calls or uploads fail the test.
pub(crate) fn run<T>(
    replies: Vec<(&'static str, Value)>,
    action: impl FnOnce(&Config) -> T,
) -> (T, Vec<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        let mut submitted = Value::Null;
        for (method, mut response) in replies {
            let deadline = Instant::now() + Duration::from_secs(10);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing RPC call {method}");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("RPC accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["method"], method);
            if method == "send_transaction" {
                submitted = request["params"].clone();
            }
            if method == "get_processed_transaction" {
                response["result"]["runtime_transaction"] = submitted.clone();
            }
            response["jsonrpc"] = json!("2.0");
            response["id"] = request["id"].clone();
            let body = response.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            requests.push(request);
        }
        requests
    });
    let config = Config {
        arch_node_url: endpoint,
        network: bitcoin::Network::Bitcoin,
        ..Config::localnet()
    };
    let result = action(&config);
    (result, server.join().unwrap())
}
