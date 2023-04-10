use reqwest::{Client, Error};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct SparseValues {
    pub indices: Vec<usize>,
    pub values: Vec<f64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Match {
    pub id: String,
    pub score: f64,
    pub values: Vec<f64>,
    pub sparseValues: Option<SparseValues>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QueryResponse {
    pub matches: Vec<Match>,
}

#[derive(Deserialize)]
pub struct UpsertResponse {
    pub upsertedCount: usize,
}

pub async fn create_index(
    pinecone_api_key: &str,
    pinecone_region: &str,
    index_name: &str,
) -> Result<(), Error> {
    let url = format!("{}/databases", get_controller_url(pinecone_region));
    let client = Client::new();
    let body = json!({
        "metric": "cosine",
        "dimension": 1536,
        "pods": 1,
        "replicas": 1,
        "pod_type": "p1.x1",
        "name": index_name
    });

    let res = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Api-Key", pinecone_api_key)
        .body(body.to_string())
        .send()
        .await?;
    Ok(())
}

pub async fn list_indexes(
    pinecone_api_key: &str,
    pinecone_region: &str,
) -> Result<Vec<String>, Error> {
    let url = format!("{}/databases", get_controller_url(pinecone_region));
    let client = Client::new();
    let res = client
        .get(&url)
        .header("Accept", "application/json; charset=utf-8")
        .header("Api-Key", pinecone_api_key)
        .send()
        .await?;
    let res2 = res.json::<Vec<String>>().await?;
    Ok(res2)
}

pub async fn query_index(
    pinecone_api_key: &str,
    pinecone_region: &str,
    project_id: &str,
    index_name: &str,
    vector: &Vec<f64>,
    top_k: &i32,
    include_metadata: &bool,
) -> Result<QueryResponse, Error> {
    let url = format!(
        "{}/query",
        get_index_url(index_name, project_id, pinecone_region)
    );
    let client = Client::new();
    let body = json!({
        "vector": vector,
        "top_k": top_k,
        "include_metadata": include_metadata,
    });

    println!("Querying Pinecone...",);

    let res = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Api-Key", pinecone_api_key)
        .body(body.to_string())
        .send()
        .await?;
    let res2 = res.json::<QueryResponse>().await?;
    Ok(res2)
}

pub async fn upsert(
    pinecone_api_key: &str,
    pinecone_region: &str,
    project_id: &str,
    index_name: &str,
    id: &str,
    vector: &Vec<f64>,
) -> Result<usize, Error> {
    let url = format!(
        "{}/vectors/upsert",
        get_index_url(index_name, project_id, pinecone_region)
    );
    let client = Client::new();
    let body = json!({
        "vectors": [{
            "id": id,
            "values": vector
        }]
    });

    println!("Storing to Pinecone...");

    let res = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Api-Key", pinecone_api_key)
        .body(body.to_string())
        .send()
        .await?;

    let res2 = res.json::<UpsertResponse>().await?;

    Ok(res2.upsertedCount)
}

fn get_index_url(index_name: &str, project_id: &str, pinecone_region: &str) -> String {
    format!(
        "https://{}-{}.svc.{}.pinecone.io",
        index_name, project_id, pinecone_region
    )
}

fn get_controller_url(pinecone_region: &str) -> String {
    format!("https://controller.{}.pinecone.io", pinecone_region)
}
