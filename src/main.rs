use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use colored::*;
use console::Term;
use dialoguer::Select;
use serde_json::{json, Value};
use std::{
    fs::File,
    io::Write,
    process::Command,
};

#[derive(Parser, Debug)]
#[command(name = "git-report")]
#[command(about = "Generate detailed commit reports from git repository")]
#[command(version)]
struct Args {
    #[arg(short, long, help = "Output file path (default: git-report-{timestamp}.txt)")]
    output: Option<String>,
    #[arg(short, long, help = "From commit hash or reference")]
    from: Option<String>,
    #[arg(short, long, help = "To commit hash or reference")]
    to: Option<String>,
    #[arg(short, long, default_value = "50", help = "Number of commits to show in selection")]
    limit: usize,
    #[arg(long, help = "Generate AI-enhanced report using local Ollama")]
    ai: bool,
    #[arg(long, default_value = "gemma3", help = "Ollama model to use for AI generation")]
    model: String,
}

#[derive(Debug, Clone)]
struct Commit {
    hash: String,
    author: String,
    date: DateTime<Utc>,
    subject: String,
    body: String,
    files_changed: Vec<String>,
}

fn check_git_repository() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to execute git command. Make sure you're in a git repository.")?;
    
    if !output.status.success() {
        anyhow::bail!("Not in a git repository");
    }
    
    let repo_path = String::from_utf8(output.stdout)?
        .trim()
        .to_string();
    
    Ok(repo_path)
}

fn get_commit_list(limit: usize) -> Result<Vec<Commit>> {
    let output = Command::new("git")
        .args([
            "log",
            "--pretty=format:%H|%an|%ad|%s",
            "--date=iso",
            &format!("-{}", limit),
        ])
        .output()
        .context("Failed to get commit list")?;
    
    if !output.status.success() {
        anyhow::bail!("Failed to get commit list");
    }
    
    let commits_str = String::from_utf8(output.stdout)?;
    let mut commits = Vec::new();
    
    for line in commits_str.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() >= 4 {
            let hash = parts[0].to_string();
            let author = parts[1].to_string();
            let date_str = parts[2];
            let subject = parts[3].to_string();
            
            let date = DateTime::parse_from_rfc3339(date_str)
                .unwrap_or_else(|_| Utc::now().into())
                .with_timezone(&Utc);
            
            let (body, files_changed) = get_commit_details(&hash)?;
            
            commits.push(Commit {
                hash,
                author,
                date,
                subject,
                body,
                files_changed,
            });
        }
    }
    
    Ok(commits)
}

fn get_commit_details(hash: &str) -> Result<(String, Vec<String>)> {
    let body_output = Command::new("git")
        .args(["show", "--no-patch", "--format=%B", hash])
        .output()
        .context("Failed to get commit body")?;
    
    let body = String::from_utf8(body_output.stdout)?
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n");
    
    let files_output = Command::new("git")
        .args(["show", "--name-only", "--format=", hash])
        .output()
        .context("Failed to get files changed")?;
    
    let files_str = String::from_utf8(files_output.stdout)?;
    let files_changed: Vec<String> = files_str
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with("commit"))
        .map(|s| s.to_string())
        .collect();
    
    Ok((body, files_changed))
}

fn select_commit<'a>(commits: &'a [Commit], prompt: &str) -> Result<&'a Commit> {
    let term = Term::stdout();
    term.clear_screen()?;
    
    println!("{}", prompt.bright_blue());
    println!("Select a commit (commits are shown in chronological order, newest first):\n");
    
    let options: Vec<String> = commits
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {} - {} ({})", i + 1, c.hash[..8].to_string(), c.subject, c.date.format("%Y-%m-%d")))
        .collect();
    
    let selection = Select::new()
        .items(&options)
        .default(0)
        .interact()
        .context("Failed to get user selection")?;
    
    Ok(&commits[selection])
}

fn get_commits_in_range(from_hash: &str, to_hash: &str) -> Result<Vec<Commit>> {
    let output = Command::new("git")
        .args([
            "log",
            "--pretty=format:%H|%an|%ad|%s",
            "--date=iso",
            &format!("{}..{}", from_hash, to_hash),
        ])
        .output()
        .context("Failed to get commits in range")?;
    
    if !output.status.success() {
        anyhow::bail!("Failed to get commits in range");
    }
    
    let commits_str = String::from_utf8(output.stdout)?;
    let mut commits = Vec::new();
    
    for line in commits_str.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() >= 4 {
            let hash = parts[0].to_string();
            let author = parts[1].to_string();
            let date_str = parts[2];
            let subject = parts[3].to_string();
            
            let date = DateTime::parse_from_rfc3339(date_str)
                .unwrap_or_else(|_| Utc::now().into())
                .with_timezone(&Utc);
            
            let (body, files_changed) = get_commit_details(&hash)?;
            
            commits.push(Commit {
                hash,
                author,
                date,
                subject,
                body,
                files_changed,
            });
        }
    }
    
    Ok(commits)
}

fn generate_report(repo_path: &str, from_commit: &Commit, to_commit: &Commit, commits: &[Commit]) -> String {
    let mut report = String::new();
    
    report.push_str(&format!("Git Commit Report\n"));
    report.push_str(&format!("================\n\n"));
    report.push_str(&format!("Repository: {}\n", repo_path));
    report.push_str(&format!("Generated: {}\n", Utc::now().format("%Y-%m-%d %H:%M:%S UTC")));
    report.push_str(&format!("Commit Range: {} -> {}\n", from_commit.hash, to_commit.hash));
    report.push_str(&format!("Total Commits: {}\n\n", commits.len()));
    
    report.push_str(&format!("Summary\n"));
    report.push_str(&format!("-------\n"));
    report.push_str(&format!("From: {} ({})\n", from_commit.subject, from_commit.hash));
    report.push_str(&format!("To: {} ({})\n", to_commit.subject, to_commit.hash));
    report.push_str(&format!("Date Range: {} to {}\n\n", 
        from_commit.date.format("%Y-%m-%d %H:%M:%S"),
        to_commit.date.format("%Y-%m-%d %H:%M:%S")));
    
    report.push_str(&format!("Detailed Commits\n"));
    report.push_str(&format!("================\n\n"));
    
    for (i, commit) in commits.iter().enumerate() {
        report.push_str(&format!("{}. {}\n", i + 1, commit.subject));
        report.push_str(&format!("   Hash: {}\n", commit.hash));
        report.push_str(&format!("   Author: {}\n", commit.author));
        report.push_str(&format!("   Date: {}\n", commit.date.format("%Y-%m-%d %H:%M:%S")));
        
        if !commit.body.trim().is_empty() {
            report.push_str(&format!("   Description:\n"));
            for line in commit.body.lines() {
                report.push_str(&format!("     {}\n", line));
            }
        }
        
        if !commit.files_changed.is_empty() {
            report.push_str(&format!("   Files Changed:\n"));
            for file in &commit.files_changed {
                report.push_str(&format!("     - {}\n", file));
            }
        }
        
        report.push_str("\n");
    }
    
    report
}

async fn generate_ai_report(repo_path: &str, from_commit: &Commit, to_commit: &Commit, commits: &[Commit], model: &str) -> Result<String> {
    // Prepare commit data for the prompt
    let mut commit_details = String::new();
    for (i, commit) in commits.iter().enumerate() {
        commit_details.push_str(&format!("Commit {}:\n", i + 1));
        commit_details.push_str(&format!("  Hash: {}\n", commit.hash));
        commit_details.push_str(&format!("  Author: {}\n", commit.author));
        commit_details.push_str(&format!("  Date: {}\n", commit.date.format("%Y-%m-%d %H:%M:%S")));
        commit_details.push_str(&format!("  Subject: {}\n", commit.subject));
        if !commit.body.trim().is_empty() {
            commit_details.push_str(&format!("  Description: {}\n", commit.body.trim()));
        }
        if !commit.files_changed.is_empty() {
            commit_details.push_str(&format!("  Files Changed:\n"));
            for file in &commit.files_changed {
                commit_details.push_str(&format!("    - {}\n", file));
            }
        }
        commit_details.push_str("\n");
    }
    
    // Prepare the prompt for Ollama
    let prompt = format!(
        "Generate a complete, professional Git commit report based on the following commit data. \
        Create a well-structured report with these sections:\n\
        1. Title and Repository Information\n\
        2. Executive Summary\n\
        3. Detailed Commit Analysis\n\
        4. Technical Impact Assessment\n\
        5. Conclusion\n\n\
        Guidelines:\n\
        - Use clear, professional language\n\
        - Avoid repetition and unnecessary sections\n\
        - Focus on what was actually accomplished\n\
        - Explain technical changes in business terms when possible\n\
        - Keep the report concise but comprehensive\n\
        - Use proper markdown formatting\n\n\
        Repository: {}\n\
        Commit Range: {} -> {}\n\
        Total Commits: {}\n\
        Generated: {}\n\n\
        Commit Data:\n{}",
        repo_path,
        from_commit.hash,
        to_commit.hash,
        commits.len(),
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        commit_details
    );
    
    // Prepare the request payload for Ollama
    let payload = json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
        "options": {
            "temperature": 0.7,
            "top_p": 0.9,
            "max_tokens": 4000
        }
    });
    
    // Make request to Ollama
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("Failed to create HTTP client")?;
    
    let response = client
        .post("http://localhost:11434/api/generate")
        .json(&payload)
        .send()
        .await
        .context(format!("Failed to connect to Ollama with model '{}'. Make sure Ollama is running on localhost:11434", model))?;
    
    if !response.status().is_success() {
        anyhow::bail!("Ollama API request failed with status: {} for model '{}'", response.status(), model);
    }
    
    let response_json: Value = response.json().await
        .context("Failed to parse Ollama response")?;
    
    let ai_report = response_json["response"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid response format from Ollama for model '{}'", model))?;
    
    Ok(ai_report.to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    println!("{}", "Git Report Generator".bright_green().bold());
    
    let repo_path = check_git_repository()?;
    println!("Repository: {}", repo_path.bright_blue());
    
    let commits = get_commit_list(args.limit)?;
    println!("Found {} commits", commits.len());
    
    let from_commit = if let Some(from) = args.from {
        commits.iter().find(|c| c.hash.starts_with(&from))
            .ok_or_else(|| anyhow::anyhow!("Commit '{}' not found", from))?
    } else {
        select_commit(&commits, "Select FROM commit (older commit)")?
    };
    
    let to_commit = if let Some(to) = args.to {
        commits.iter().find(|c| c.hash.starts_with(&to))
            .ok_or_else(|| anyhow::anyhow!("Commit '{}' not found", to))?
    } else {
        select_commit(&commits, "Select TO commit (newer commit)")?
    };
    
    println!("Range: {} -> {}", from_commit.subject, to_commit.subject);
    
    let range_commits = get_commits_in_range(&from_commit.hash, &to_commit.hash)?;
    println!("Found {} commits in range", range_commits.len());
    
    let report_content = if args.ai {
        println!("{}", format!("Generating AI-enhanced report using Ollama with model '{}'...", args.model).blue());
        generate_ai_report(&repo_path, from_commit, to_commit, &range_commits, &args.model).await?
    } else {
        generate_report(&repo_path, from_commit, to_commit, &range_commits)
    };
    
    let output_file = args.output.unwrap_or_else(|| {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let suffix = if args.ai { "-ai" } else { "" };
        format!("git-report{}-{}.txt", suffix, timestamp)
    });
    
    let mut file = File::create(&output_file)?;
    file.write_all(report_content.as_bytes())?;
    
    println!("Report saved to: {}", output_file.bright_blue());
    
    Ok(())
}
