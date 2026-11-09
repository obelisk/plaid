use std::fs;

use warp::Filter;

#[tokio::main]
async fn main() {
    //println!("Starting handler server...");
    // Define the route for handling POST requests
    let post_route = warp::post()
        .and(warp::path("response"))
        .and(warp::body::bytes())
        .map(|body: warp::hyper::body::Bytes| {
            //println!("POST Received");
            let body_str = String::from_utf8(body.to_vec()).unwrap();
            println!("{body_str}");
            warp::reply::with_status("", warp::http::StatusCode::OK)
        });

    // The cron route writes to stderr so as to not interfere with other tests.
    let cron_route = warp::post()
        .and(warp::path("testcron"))
        .and(warp::body::bytes())
        .map(|body: warp::hyper::body::Bytes| {
            let body_str = String::from_utf8(body.to_vec()).unwrap();
            eprintln!("{body_str}");
            warp::reply::with_status("", warp::http::StatusCode::OK)
        });

    let mnr_route = warp::post()
        .and(warp::path("testmnr"))
        .and(warp::body::bytes())
        .map(|body: warp::hyper::body::Bytes| {
            let body_str = String::from_utf8(body.to_vec()).unwrap();
            println!("{body_str} from /testmnr");
            warp::reply::with_status("", warp::http::StatusCode::OK)
        });

    let mnr_vars_route = warp::post()
        .and(warp::path!("testmnr" / "my_variable"))
        .and(warp::body::bytes())
        .map(|body: warp::hyper::body::Bytes| {
            let body_str = String::from_utf8(body.to_vec()).unwrap();
            println!("{body_str} from /testmnr/my_variable");
            warp::reply::with_status("", warp::http::StatusCode::OK)
        });

    let mnr_headers_route = warp::post()
        .and(warp::path!("testmnr" / "headers"))
        .and(warp::body::bytes())
        .and(warp::header::headers_cloned())
        .map(
            |body: warp::hyper::body::Bytes, headers: warp::http::HeaderMap| {
                let body_str = String::from_utf8(body.to_vec()).unwrap();
                // Get the headers and see if they match expected values. If not, print a NOK which
                // will eventually show up when comparing this server's output with the expected output.
                if headers.get("first_header").unwrap().to_str().unwrap() != "first_value" {
                    println!("NOK");
                }
                if headers.get("second_header").unwrap().to_str().unwrap() != "second_value" {
                    println!("NOK");
                }
                println!("{body_str} from /testmnr/headers");
                warp::reply::with_status("", warp::http::StatusCode::OK)
            },
        );

    let cert = fs::read("/tmp/plaid_config/server.pem").expect("failed to read server.pem");
    let key = fs::read("/tmp/plaid_config/server.key").expect("failed to read server.key");

    // Start the server on 127.0.0.1:8998
    let routes = post_route
        .or(cron_route)
        .or(mnr_vars_route)
        .or(mnr_headers_route)
        .or(mnr_route);

    // Shut down gracefully on SIGTERM/SIGINT. The handlers print a line before
    // the HTTP reply is sent, so a hard kill can interrupt a response that is
    // still in flight. That makes the rule's named request fail even though the
    // output the test asserts on was already written. Draining in-flight
    // requests before exiting keeps the two consistent.
    let (_, server) = warp::serve(routes)
        .tls()
        .cert(cert)
        .key(key)
        .bind_with_graceful_shutdown(([127, 0, 0, 1], 8998), async {
            shutdown_signal().await;
        });

    server.await;
}

/// Resolve when the process is asked to terminate.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
