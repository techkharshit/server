use aws_sdk_s3::{ByteStream, Client, Credentials, Endpoint, Region, SdkError}; // Added SdkError import
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::config::Builder as S3ConfigBuilder;
use aws_sdk_s3::error::HeadBucketError;
use bytes::Bytes;
use dotenv::dotenv;
use std::env;
use std::fs;
use std::path::Path;
use warp::Filter;
use warp::http::StatusCode;
use warp::Reply;
use sqlx::mysql::MySqlPoolOptions;
use sqlx::MySql;
use sqlx::Row;
// use serde_json::Value; // Remove this line if not used

const DOWNLOADS_DIR: &str = "/usr/src/app/Downloads";

#[tokio::main]
async fn main() {
    dotenv().ok();

    let minio_endpoint = env::var("MINIO_ENDPOINT").expect("MINIO_ENDPOINT must be set");
    let _minio_bucket = env::var("MINIO_BUCKET").expect("MINIO_BUCKET must be set");
    let minio_access_key = env::var("MINIO_ACCESS_KEY").expect("MINIO_ACCESS_KEY must be set");
    let minio_secret_key = env::var("MINIO_SECRET_KEY").expect("MINIO_SECRET_KEY must be set");
    let mysql_url = env::var("MYSQL_URL").expect("MYSQL_URL must be set");

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

    let mysql_pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(&mysql_url)
        .await
        .expect("Failed to create MySQL pool");

    ensure_table_exists(&mysql_pool).await.expect("Failed to ensure table exists");

    let mysql_pool_filter = warp::any().map(move || mysql_pool.clone());

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

    let put_mysql_route = warp::post()
        .and(warp::path("put-mysql"))
        .and(warp::body::bytes())
        .and(mysql_pool_filter.clone())
        .and_then(put_mysql);

    let get_mysql_route = warp::get()
        .and(warp::path!("get-mysql" / String))
        .and(mysql_pool_filter.clone())
        .and_then(get_mysql);

    let routes = put_local_route
        .or(get_local_route)
        .or(put_s3_route)
        .or(get_s3_route)
        .or(put_mysql_route)
        .or(get_mysql_route);

    warp::serve(routes).run(([0, 0, 0, 0], 8000)).await;
}

async fn ensure_table_exists(pool: &sqlx::Pool<MySql>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS datasets (
            id INT AUTO_INCREMENT PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            data LONGTEXT NOT NULL
        )"
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn put_local(file_name: Bytes) -> Result<impl warp::Reply, warp::Rejection> {
    let file_name_str = match std::str::from_utf8(&file_name) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Invalid UTF-8 sequence: {}", e);
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Invalid UTF-8 sequence"),
                StatusCode::BAD_REQUEST,
            ));
        }
    };

    let path = Path::new(DOWNLOADS_DIR).join(file_name_str);
    println!("Attempting to read file from path: {:?}", path);

    if let Ok(entries) = fs::read_dir(DOWNLOADS_DIR) {
        println!("Contents of the Downloads directory:");
        for entry in entries {
            if let Ok(entry) = entry {
                println!("Found file: {:?}", entry.path());
            }
        }
    } else {
        println!("Failed to read the Downloads directory.");
    }

    if !path.exists() {
        eprintln!("File not found: {:?}", path);
        return Ok(warp::reply::with_status(
            warp::reply::json(&"File not found"),
            StatusCode::BAD_REQUEST,
        ));
    }

    let contents = match fs::read_to_string(&path) {
        Ok(contents) => {
            println!("File contents: {}", contents);
            contents
        }
        Err(e) => {
            eprintln!("Failed to read file: {}", e);
            return Ok(warp::reply::with_status(
                warp::reply::json(&format!("Failed to read file: {}", e)),
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };

    let dest_path = Path::new("local_store").join(file_name_str);
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

    println!("File put locally: {}", file_name_str);
    Ok(warp::reply::with_status(
        warp::reply::json(&format!("File put locally: {}", file_name_str)),
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

async fn put_s3(file_name: Bytes, s3_client: Client) -> Result<impl warp::Reply, warp::Rejection> {
    let minio_bucket = env::var("MINIO_BUCKET").expect("MINIO_BUCKET must be set");

    let file_name_str = match std::str::from_utf8(&file_name) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Invalid UTF-8 sequence: {}", e);
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Invalid UTF-8 sequence"),
                StatusCode::BAD_REQUEST,
            ));
        }
    };

    let path = Path::new(DOWNLOADS_DIR).join(file_name_str);
    println!("Attempting to read file from path: {:?}", path);

    if let Ok(entries) = fs::read_dir(DOWNLOADS_DIR) {
        println!("Contents of the Downloads directory:");
        for entry in entries {
            if let Ok(entry) = entry {
                println!("Found file: {:?}", entry.path());
            }
        }
    } else {
        println!("Failed to read the Downloads directory.");
    }

    if !path.exists() {
        eprintln!("File not found: {:?}", path);
        return Ok(warp::reply::with_status(
            warp::reply::json(&"File not found"),
            StatusCode::BAD_REQUEST,
        ));
    }

    let contents = match fs::read_to_string(&path) {
        Ok(contents) => {
            println!("File contents: {}", contents);
            contents
        }
        Err(e) => {
            eprintln!("Failed to read file: {}", e);
            return Ok(warp::reply::with_status(
                warp::reply::json(&format!("Failed to read file: {}", e)),
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };

    // Check if the bucket exists
    let bucket_exists = s3_client.head_bucket()
        .bucket(&minio_bucket)
        .send()
        .await
        .is_ok();

    if !bucket_exists {
        // Create the bucket if it does not exist
        match s3_client.create_bucket()
            .bucket(&minio_bucket)
            .send()
            .await {
            Ok(_) => println!("Bucket created: {}", minio_bucket),
            Err(e) => {
                eprintln!("Failed to create bucket: {:?}", e);
                return Ok(warp::reply::with_status(
                    warp::reply::json(&format!("Failed to create bucket: {:?}", e)),
                    StatusCode::INTERNAL_SERVER_ERROR,
                ));
            }
        }
    } else {
        println!("Bucket already exists: {}", minio_bucket);
    }

    // Upload the file to the bucket
    let put_req = s3_client.put_object()
        .bucket(&minio_bucket)
        .key(file_name_str)
        .body(ByteStream::from(contents.into_bytes()));

    match put_req.send().await {
        Ok(_) => {
            println!("File put to S3: {}", file_name_str);
            Ok(warp::reply::with_status(
                warp::reply::json(&format!("File put to S3: {}", file_name_str)),
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
        .key(&file_name);

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

async fn put_mysql(file_name: Bytes, pool: sqlx::Pool<MySql>) -> Result<impl warp::Reply, warp::Rejection> {
    let file_name_str = match std::str::from_utf8(&file_name) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Invalid UTF-8 sequence: {}", e);
            return Ok(warp::reply::with_status(
                warp::reply::json(&"Invalid UTF-8 sequence"),
                StatusCode::BAD_REQUEST,
            ));
        }
    };

    let path = Path::new(DOWNLOADS_DIR).join(file_name_str);
    println!("Attempting to read file from path: {:?}", path);

    if let Ok(entries) = fs::read_dir(DOWNLOADS_DIR) {
        println!("Contents of the Downloads directory:");
        for entry in entries {
            if let Ok(entry) = entry {
                println!("Found file: {:?}", entry.path());
            }
        }
    } else {
        println!("Failed to read the Downloads directory.");
    }

    if !path.exists() {
        eprintln!("File not found: {:?}", path);
        return Ok(warp::reply::with_status(
            warp::reply::json(&"File not found"),
            StatusCode::BAD_REQUEST,
        ));
    }

    let contents = match fs::read_to_string(&path) {
        Ok(contents) => {
            println!("File contents: {}", contents);
            contents
        }
        Err(e) => {
            eprintln!("Failed to read file: {}", e);
            return Ok(warp::reply::with_status(
                warp::reply::json(&format!("Failed to read file: {}", e)),
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };

    let query = sqlx::query("INSERT INTO datasets (name, data) VALUES (?, ?)")
        .bind(file_name_str)
        .bind(contents);

    match query.execute(&pool).await {
        Ok(_) => {
            println!("File put to MySQL: {}", file_name_str);
            Ok(warp::reply::with_status(
                warp::reply::json(&format!("File put to MySQL: {}", file_name_str)),
                StatusCode::OK,
            ))
        }
        Err(e) => {
            eprintln!("Failed to put file to MySQL: {:?}", e);
            let error_message = warp::reply::json(&format!("Failed to put file to MySQL: {:?}", e));
            Ok(warp::reply::with_status(
                error_message,
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

async fn get_mysql(file_name: String, pool: sqlx::Pool<MySql>) -> Result<impl warp::Reply, warp::Rejection> {
    let query = sqlx::query("SELECT data FROM datasets WHERE name = ?")
        .bind(&file_name);

    match query.fetch_one(&pool).await {
        Ok(row) => {
            let json_data: String = match row.try_get("data") {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("Failed to decode TEXT column: {:?}", e);
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&format!("Failed to decode TEXT column: {:?}", e)),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    ));
                }
            };
            println!("File contents from MySQL:\n{}", json_data);
            Ok(warp::reply::with_status(
                warp::reply::json(&json_data),
                StatusCode::OK,
            ))
        }
        Err(sqlx::Error::RowNotFound) => {
            eprintln!("File not found in MySQL: {}", file_name);
            Ok(warp::reply::with_status(
                warp::reply::json(&format!("File not found in MySQL: {}", file_name)),
                StatusCode::NOT_FOUND,
            ))
        }
        Err(e) => {
            eprintln!("Failed to get file from MySQL: {:?}", e);
            let error_message = warp::reply::json(&format!("Failed to get file from MySQL: {:?}", e));
            Ok(warp::reply::with_status(
                error_message,
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}
