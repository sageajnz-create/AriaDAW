// Standalone probe: does a tiny_http server + reqwest blocking client
// complete a POST round-trip on this machine? Run with `cargo script`-style:
//   rustc probe.rs && ./probe.exe
use std::io::Read;

fn main() {
    let server = tiny_http::Server::http("127.0.0.1:8980").unwrap();
    let t = std::thread::spawn(move || {
        for req in server.incoming_requests() {
            println!("server: got {} {}", req.method(), req.url());
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);
            println!("server: body {} bytes, replying", body.len());
            let resp = tiny_http::Response::from_string("{\"id\":\"x\"}").with_status_code(400u16);
            let _ = req.respond(resp);
        }
    });

    let client = reqwest::blocking::Client::new();
    let started = std::time::Instant::now();
    let res = client
        .post("http://127.0.0.1:8980/synth")
        .json(&serde_json::json!({}))
        .timeout(std::time::Duration::from_secs(5))
        .send();
    match res {
        Ok(r) => println!("client: status {} in {:?}", r.status(), started.elapsed()),
        Err(e) => println!("client: error {e} after {:?}", started.elapsed()),
    }
    drop(t);
}
