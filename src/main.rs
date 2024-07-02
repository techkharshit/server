use bytes::Bytes;
use rusoto_core::{HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_s3::{GetObjectRequest, PutObjectRequest, S3Client, S3};
use std::fs;
use std::path::Path;
use tokio::io::AsyncReadExt;
use warp::Filter;
use warp::http::StatusCode;

const MINIO_ENDPOINT: &str = "http://localhost:9000";
const MINIO_BUCKET: &str = "test";
const MINIO_ACCESS_KEY: &str = "9AltFABzTmVB1ZH6G9mQ";
const MINIO_SECRET_KEY: &str = "MEYaTADr4575Ngw4iHgJGxt134otk5JHC49VFlc3";

#[tokio::main]
async fn main() {
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
        .and_then(put_s3);

    let get_s3_route = warp::get()
        .and(warp::path!("get-s3" / String))
        .and_then(get_s3);

    let routes = put_local_route
        .or(get_local_route)
        .or(put_s3_route)
        .or(get_s3_route);

    warp::serve(routes).run(([127, 0, 0, 1], 8000)).await;
}

async fn put_local(file_path: Bytes) -> Result<impl warp::Reply, warp::Rejection> {
    let file_path_str = std::str::from_utf8(&file_path).unwrap();
    let path = Path::new(file_path_str);
    let file_name = path.file_name().unwrap().to_str().unwrap();

    let contents = fs::read_to_string(file_path_str).unwrap();
    let dest_path = Path::new("local_store").join(file_name);
    fs::create_dir_all(dest_path.parent().unwrap()).unwrap();
    fs::write(dest_path, contents).unwrap();
    println!("File put locally: {}", file_name);
    Ok(warp::reply::with_status(
        warp::reply::json(&"File put locally"),
        StatusCode::OK,
    ))
}

async fn get_local(file_name: String) -> Result<impl warp::Reply, warp::Rejection> {
    let file_path = Path::new("local_store").join(file_name);
    let contents = match fs::read_to_string(&file_path) {
        Ok(contents) => contents,
        Err(_) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&"File not found"),
                StatusCode::NOT_FOUND,
            ))
        }
    };
    println!("File contents:\n{}", contents);
    Ok(warp::reply::with_status(
        warp::reply::json(&contents),
        StatusCode::OK,
    ))
}

async fn put_s3(file_path: Bytes) -> Result<impl warp::Reply, warp::Rejection> {
    let file_path_str = std::str::from_utf8(&file_path).unwrap();
    let path = Path::new(file_path_str);
    let file_name = path.file_name().unwrap().to_str().unwrap();
    let contents = fs::read_to_string(file_path_str).unwrap();

    let credentials = StaticProvider::new(MINIO_ACCESS_KEY.to_string(), MINIO_SECRET_KEY.to_string(), None, None);
    let region = Region::Custom {
        name: "us-east-1".to_owned(),
        endpoint: MINIO_ENDPOINT.to_owned(),
    };
    let client = S3Client::new_with(HttpClient::new().unwrap(), credentials, region);

    let put_req = PutObjectRequest {
        bucket: MINIO_BUCKET.to_string(),
        key: file_name.to_string(),
        body: Some(contents.into_bytes().into()),
        ..Default::default()
    };

    match client.put_object(put_req).await {
        Ok(_) => {
            println!("File put to S3: {}", file_name);
            Ok(warp::reply::with_status(
                warp::reply::json(&"File put to S3"),
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

async fn get_s3(file_name: String) -> Result<impl warp::Reply, warp::Rejection> {
    let credentials = StaticProvider::new(MINIO_ACCESS_KEY.to_string(), MINIO_SECRET_KEY.to_string(), None, None);
    let region = Region::Custom {
        name: "us-east-1".to_owned(),
        endpoint: MINIO_ENDPOINT.to_owned(),
    };
    let client = S3Client::new_with(HttpClient::new().unwrap(), credentials, region);

    let get_req = GetObjectRequest {
        bucket: MINIO_BUCKET.to_string(),
        key: file_name.to_string(),
        ..Default::default()
    };

    match client.get_object(get_req).await {
        Ok(result) => {
            let mut stream = result.body.unwrap().into_async_read();
            let mut contents = String::new();
            stream.read_to_string(&mut contents).await.unwrap();
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
