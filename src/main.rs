use aws_sdk_s3::{ByteStream, Client, Credentials, Endpoint, Region};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::config::Builder as S3ConfigBuilder;
use bytes::Bytes;
use std::fs;
use std::path::Path;
use warp::Filter;
use warp::http::StatusCode;
use std::env;
use dotenv::dotenv;

#[tokio::main]
async fn main() {
    dotenv().ok();

    let minio_endpoint = env::var("MINIO_ENDPOINT").expect("MINIO_ENDPOINT must be set");
    let minio_bucket = env::var("MINIO_BUCKET").expect("MINIO_BUCKET must be set");
    let minio_access_key = env::var("MINIO_ACCESS_KEY").expect("MINIO_ACCESS_KEY must be set");
    let minio_secret_key = env::var("MINIO_SECRET_KEY").expect("MINIO_SECRET_KEY must be set");

    let region_provider = RegionProviderChain::default_provider().or_else(Region::new("us-east-1"));
    let base_config = aws_config::from_env()
        .region(region_provider)
        .credentials_provider(Credentials::new(
            &minio_access_key,
            &minio_secret_key,
            None,
            None,
            "loaded-from-custom-config",
        ))
        .load()
        .await;

    let config = S3ConfigBuilder::from(&base_config)
        .endpoint_resolver(Endpoint::immutable(minio_endpoint.parse().unwrap()))
        .build();

    let s3_client = Client::from_conf(config);
    let s3_client_filter = warp::any().map(move || s3_client.clone());

    let put_local_route = warp::post()
        .and(warp::path("put-local"))
        .and(warp::body::bytes())
        .and_then(put_local);

    let get_local_route = warp::get()
        .and(warp::path!("get-local" / String))
        .and_then(get_local);

    let put_s3_route = warp::post()
        .and(warp::path("put-s3"))
        .and(warp::body::bytes())
        .and(s3_client_filter.clone())
        .and_then(put_s3);

    let get_s3_route = warp::get()
        .and(warp::path!("get-s3" / String))
        .and(s3_client_filter.clone())
        .and_then(get_s3);

    let routes = put_local_route
        .or(get_local_route)
        .or(put_s3_route)
        .or(get_s3_route);

    warp::serve(routes).run(([0, 0, 0, 0], 8000)).await;
}

async fn put_local(file_path: Bytes) -> Result<impl warp::Reply, warp::Rejection> {
    let file_path_str = match std::str::from_utf8(&file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Invalid UTF-8 sequence: {}", e);
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Invalid UTF-8 sequence"),
                StatusCode::BAD_REQUEST,
            ));
        }
    };

    let path = Path::new(file_path_str);
    let file_name = match path.file_name() {
        Some(name) => name.to_str().expect("Invalid file name"),
        None => {
            eprintln!("Invalid file path: {:?}", path);
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Invalid file path"),
                StatusCode::BAD_REQUEST,
            ));
        }
    };

    let contents = match fs::read_to_string(file_path_str) {
        Ok(contents) => contents,
        Err(e) => {
            eprintln!("Failed to read file: {}", e);
            return Ok(warp::reply::with_status(
                warp::reply::json(&format!("Failed to read file: {}", e)),
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };

    let dest_path = Path::new("local_store").join(file_name);
    if let Err(e) = fs::create_dir_all(dest_path.parent().unwrap()) {
        eprintln!("Failed to create directories: {}", e);
        return Ok(warp::reply::with_status(
            warp::reply::json(&format!("Failed to create directories: {}", e)),
            StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    if let Err(e) = fs::write(&dest_path, contents) {
        eprintln!("Failed to write file: {}", e);
        return Ok(warp::reply::with_status(
            warp::reply::json(&format!("Failed to write file: {}", e)),
            StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    println!("File put locally: {}", file_name);
    Ok(warp::reply::with_status(
        warp::reply::json(&format!("File put locally: {}", file_name)),
        StatusCode::OK,
    ))
}

async fn get_local(file_name: String) -> Result<impl warp::Reply, warp::Rejection> {
    let file_path = Path::new("local_store").join(&file_name);
    let contents = match fs::read_to_string(&file_path) {
        Ok(contents) => contents,
        Err(e) => {
            eprintln!("Failed to read file: {}", e);
            return Ok(warp::reply::with_status(
                warp::reply::json(&format!("Failed to read file: {}", e)),
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };

    println!("File contents from local store:\n{}", contents);
    Ok(warp::reply::with_status(
        warp::reply::json(&contents),
        StatusCode::OK,
    ))
}

async fn put_s3(file_path: Bytes, s3_client: Client) -> Result<impl warp::Reply, warp::Rejection> {
    let minio_bucket = env::var("MINIO_BUCKET").expect("MINIO_BUCKET must be set");

    let file_path_str = match std::str::from_utf8(&file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Invalid UTF-8 sequence: {}", e);
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Invalid UTF-8 sequence"),
                StatusCode::BAD_REQUEST,
            ));
        }
    };

    let path = Path::new(file_path_str);
    let file_name = match path.file_name() {
        Some(name) => name.to_str().expect("Invalid file name"),
        None => {
            eprintln!("Invalid file path: {:?}", path);
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Invalid file path"),
                StatusCode::BAD_REQUEST,
            ));
        }
    };

    let contents = match fs::read_to_string(file_path_str) {
        Ok(contents) => contents,
        Err(e) => {
            eprintln!("Failed to read file: {}", e);
            return Ok(warp::reply::with_status(
                warp::reply::json(&format!("Failed to read file: {}", e)),
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };

    let put_req = s3_client.put_object()
        .bucket(&minio_bucket)
        .key(file_name)
        .body(ByteStream::from(contents.into_bytes()));

    match put_req.send().await {
        Ok(_) => {
            println!("File put to S3: {}", file_name);
            Ok(warp::reply::with_status(
                warp::reply::json(&format!("File put to S3: {}", file_name)),
                StatusCode::OK,
            ))
        }
        Err(e) => {
            eprintln!("Failed to put file to S3: {:?}", e);
            let error_message = warp::reply::json(&format!("Failed to put file to S3: {:?}", e));
            Ok(warp::reply::with_status(
                error_message,
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

async fn get_s3(file_name: String, s3_client: Client) -> Result<impl warp::Reply, warp::Rejection> {
    let minio_bucket = env::var("MINIO_BUCKET").expect("MINIO_BUCKET must be set");

    let get_req = s3_client.get_object()
        .bucket(&minio_bucket)
        .key(file_name);

    match get_req.send().await {
        Ok(result) => {
            let data = result.body.collect().await.unwrap();
            let bytes = data.into_bytes();
            let contents = String::from_utf8_lossy(&bytes);
            println!("File contents from S3:\n{}", contents);
            Ok(warp::reply::with_status(
                warp::reply::json(&contents),
                StatusCode::OK,
            ))
        }
        Err(e) => {
            eprintln!("Failed to get file from S3: {:?}", e);
            let error_message = warp::reply::json(&format!("Failed to get file from S3: {:?}", e));
            Ok(warp::reply::with_status(
                error_message,
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}
