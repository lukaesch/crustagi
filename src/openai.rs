use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::time::sleep;

use crate::load_env_var;

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    data: Vec<EmbeddingData>,
    model: String,
    object: String,
    usage: Usage,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingData {
    pub embedding: Vec<f64>,
    pub index: usize,
    pub object: String,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: usize,
    total_tokens: usize,
}

// Get embedding using OpenAI API
pub async fn get_ada_embedding(
    api_key: &str,
    text: &str,
) -> Result<EmbeddingData, Box<dyn std::error::Error>> {
    let text = text.replace('\n', " ");
    let input = serde_json::json!({
        "input": text,
        "model": "text-embedding-ada-002"
    });

    let res: Value = openai_call_api(api_key, "embeddings", input).await.into();
    let embedding_data: EmbeddingData = serde_json::from_value(res["data"][0].clone())?;
    Ok(embedding_data)
}

// Call OpenAI API with completion or chat completion
pub async fn openai_call(openai_api_key: &str, prompt: &str) -> String {
    let openai_api_model = load_env_var("OPENAI_API_MODEL");
    let (endpoint, input, is_chat_completion) = if openai_api_model.starts_with("gpt-") {
        // Use chat completion API
        let messages = vec![json!({ "role": "user", "content": prompt })];
        let input = json!({
            "model": openai_api_model,
            "messages": messages,
            "temperature": 0.5,
            "max_tokens": 100,
            "n": 1,
            "stop": Value::Null,
        });
        ("chat/completions", input, true)
    } else {
        // Use completion API
        let input = json!({
            "model": openai_api_model,
            "prompt": prompt,
            "temperature": 0.7,
            "max_tokens": 2000,
        });
        ("completions", input, false)
    };

    let res = openai_call_api(openai_api_key, endpoint, input).await;
    let text = if is_chat_completion {
        // Extract content from "message" field for chat completion
        res["choices"][0]["message"]["content"].as_str().unwrap()
    } else {
        // Extract content from "text" field for completion
        res["choices"][0]["text"].as_str().unwrap()
    };

    text.to_string()
}

// Call OpenAI API
pub async fn openai_call_api(
    openai_api_key: &str,
    endpoint: &str,
    input: serde_json::Value,
) -> serde_json::Value {
    let openai_url = format!("https://api.openai.com/v1/{}", endpoint);

    loop {
        println!("Calling OpenAI API...");
        let client = reqwest::Client::new();
        let res = client
            .post(&openai_url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", openai_api_key))
            .body(input.to_string())
            .send()
            .await
            .unwrap();
        if res.status().is_success() {
            let res: serde_json::Value = res.json().await.unwrap();
            return res;
        } else if res.status().as_u16() == 429 {
            println!(
                "The OpenAI API rate limit has been exceeded. Waiting 10 seconds and trying again."
            );
            sleep(Duration::from_secs(10)).await;
        }
    }
}


