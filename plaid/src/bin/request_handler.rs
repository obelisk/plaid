use warp::Filter;

#[tokio::main]
async fn main() {
    // Define the route for handling POST requests
    let post_route = warp::post()
        .and(warp::path("response"))
        .and(warp::body::bytes())
        .map(|body: warp::hyper::body::Bytes| {
            let body_str = String::from_utf8(body.to_vec()).unwrap();
            println!("{body_str}");
            warp::reply::with_status("", warp::http::StatusCode::OK)
        });

    // Start the server on 127.0.0.1:8998
    warp::serve(post_route).run(([127, 0, 0, 1], 8998)).await;
}
