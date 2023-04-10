mod openai;
mod pinecone;

use dotenv::dotenv;
use pinecone::{create_index, list_indexes, query_index, upsert};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::env;
use std::time::Duration;
use tokio::time::sleep;

use crate::openai::{get_ada_embedding, openai_call};

struct Config {
    openai_api_key: String,
    pinecone_api_key: String,
    pinecone_region: String,
    pinecone_project_id: String,
    pinecone_index_name: String,
    initial_task: String,
    objective: String,
}

// Data structure for tasks
#[derive(Debug, Serialize, Deserialize)]
struct Task {
    task_id: i32,
    task_name: String,
}

// Load environment variables
fn load_env_var(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{} environment variable is missing", name))
}

// Task creation agent
async fn task_creation_agent(
    api_key: &str,
    objective: &str,
    result: &str,
    task_description: &str,
    incompleted_task_list: &mut VecDeque<Task>,
) -> Vec<Task> {
    let prompt = format!(
        r#"
        You are an task creation AI that uses the result of an execution agent to create new tasks with the following objective: {}.
        The last completed task has the result: {}.
        This result was based on this task description: {}. These are incomplete tasks: {:?}.
        Based on the result, create new tasks to be completed by the AI system that do not overlap with incomplete tasks.
        Return the tasks as an array."#,
        objective, result, task_description, incompleted_task_list
    );

    let response = openai_call(api_key, &prompt).await;
    let new_tasks = response.trim().split('\n').map(|t| {
        // Extract only the task description (after the dot) and trim any leading/trailing whitespace
        let task_description = t.splitn(2, '.').nth(1).map(|s| s.trim().to_string());
        Task {
            task_id: 0,
            task_name: task_description.unwrap_or_else(|| "".to_string()),
        }
    });
    new_tasks.collect()
}

// Task prioritization agent
async fn prioritization_agent(
    openai_api_key: &str,
    objective: &str,
    task_list: &mut VecDeque<Task>,
    task_id: &i32,
) {
    let task_names: Vec<&str> = task_list.iter().map(|t| t.task_name.as_str()).collect();
    let prompt = format!(
        r#"
        You are an task prioritization AI tasked with cleaning the formatting of and reprioritizing the following tasks: {:?}.
        Consider the ultimate objective of your team:{}.
        Do not remove any tasks. Return the result as a numbered list, like:
        #. First task
        #. Second task
        Start the task list with number {}."#,
        task_names, objective, task_id
    );

    let response = openai_call(openai_api_key, &prompt).await;
    task_list.clear();
    for task_string in response.trim().split('\n') {
        if let Some(task_name) = task_string
            .trim()
            .splitn(2, '.')
            .nth(1)
            .map(|s| s.trim().to_string())
        {
            let task_id = task_list.back().map_or(1, |t| t.task_id + 1);
            task_list.push_back(Task { task_id, task_name });
        }
    }
}

// Execution agent
async fn execution_agent(config: &Config, task: &Task) -> Result<String, reqwest::Error> {
    println!("Executing task: {}...", task.task_name);
    let context = context_agent(&config, &config.objective, 5).await?;
    let context_str = context.join("\n");
    let prompt = format!(
        r#"
        You are an AI who performs one task based on the following objective: {}.
        Take into account these previously completed tasks: {}.
        Your task: {}.
        Response:"#,
        &config.objective, context_str, task.task_name
    );

    Ok(openai_call(&config.openai_api_key, &prompt).await)
}

// Context agent
async fn context_agent(
    config: &Config,
    query: &str,
    n: i32,
) -> Result<Vec<String>, reqwest::Error> {
    println!("Getting context...");
    let query_embedding = get_ada_embedding(&config.openai_api_key, query).await;

    let query_index_result = query_index(
        &config.pinecone_api_key,
        &config.pinecone_region,
        &config.pinecone_project_id,
        &config.pinecone_index_name,
        &query_embedding.unwrap().embedding,
        &n,
        &true,
    )
    .await?;
    // Collect the matches into a Vec and sort it
    let mut sorted_results = query_index_result.matches;
    sorted_results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Map the sorted results to extract the "task" metadata and collect into a Vec
    let tasks: Vec<String> = sorted_results
        .into_iter()
        .filter_map(|item| {
            item.metadata
                .as_ref() // Convert Option<&HashMap> to Option<&HashMap>
                .and_then(|metadata| metadata.get("task")) // Extract "task" if metadata exists
                .map(|v| v.to_string()) // Convert the value to a string
        })
        .collect();

    Ok(tasks)
}

// Add a task to the list
fn add_task(task: Task, task_list: &mut VecDeque<Task>) {
    println!("Adding task: {}...", task.task_name);
    task_list.push_back(task);
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    // // Set config
    let config = Config {
        openai_api_key: load_env_var("OPENAI_API_KEY"),
        pinecone_api_key: load_env_var("PINECONE_API_KEY"),
        pinecone_region: load_env_var("PINECONE_REGION"),
        pinecone_project_id: load_env_var("PINECONE_PROJECT_ID"),
        pinecone_index_name: load_env_var("PINECONE_INDEX_NAME"),
        initial_task: load_env_var("INITIAL_TASK"),
        objective: load_env_var("OBJECTIVE"),
    };

    // // Set Pinecone index
    let indexes = list_indexes(&config.pinecone_api_key, &config.pinecone_region)
        .await
        .unwrap();
    if !indexes.contains(&config.pinecone_index_name) {
        create_index(
            &config.pinecone_api_key,
            &config.pinecone_region,
            &config.pinecone_index_name,
        )
        .await
        .unwrap();
    }

    // // Create task list
    let mut task_list = VecDeque::new();
    let first_task = Task {
        task_id: 1,
        task_name: config.initial_task.clone(),
    };
    add_task(first_task, &mut task_list);

    // // Main loop
    let mut task_id_counter = 1;
    loop {
        if !task_list.is_empty() {
            // Print the task list
            println!("\n*****TASK LIST*****");
            for t in &task_list {
                println!("{}: {}", t.task_id, t.task_name);
            }

            // Step 1: Pull the first task
            let task = task_list.pop_front().unwrap();
            println!("\n*****NEXT TASK*****");
            println!("{}: {}", task.task_id, task.task_name);

            let result = execution_agent(&config, &task).await;

            let result_ref = result.as_ref().unwrap();

            let this_task_id = task.task_id;
            println!("\n*****TASK RESULT*****");
            println!("{}", result_ref);

            // Step 2: Enrich result and store in Pinecone
            // This is where you should enrich the result if needed
            let result_id = format!("result_{}", task.task_id);
            let vector = get_ada_embedding(&config.openai_api_key, result_ref).await;
            upsert(
                &config.pinecone_api_key,
                &config.pinecone_region,
                &config.pinecone_project_id,
                &config.pinecone_index_name,
                &result_id,
                vector.unwrap().embedding.as_ref(),
            )
            .await
            .unwrap();

            // Step 3: Create new tasks and reprioritize task list
            let new_tasks = task_creation_agent(
                &config.openai_api_key,
                &config.objective,
                result_ref,
                &task.task_name,
                &mut task_list,
            )
            .await;
            for new_task in new_tasks {
                task_id_counter += 1;
                let task = Task {
                    task_id: task_id_counter,
                    task_name: new_task.task_name.clone(),
                };
                add_task(task, &mut task_list);
            }
            // Step 4: Reprioritize the task list
            prioritization_agent(
                &config.openai_api_key,
                &config.objective,
                &mut task_list,
                &this_task_id,
            )
            .await;
        }
        sleep(Duration::from_secs(1)).await; // Sleep before checking the task list again
    }
}
