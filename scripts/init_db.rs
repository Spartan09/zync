#!/usr/bin/env -S cargo +nightly -Zscript

---cargo
 [package]
 edition = "2024"
---
use std::env;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn main() {
    let db_port = env::var("DB_PORT").unwrap_or_else(|_| "5432".to_string());
    let app_user = env::var("APP_USER").unwrap_or_else(|_| "app".to_string());
    let app_user_pwd = env::var("APP_USER_PWD").unwrap_or_else(|_| "secret".to_string());
    let app_db_name = env::var("APP_DB_NAME").unwrap_or_else(|_| "newsletter".to_string());
    let superuser = env::var("SUPERUSER").unwrap_or_else(|_| "postgres".to_string());
    let superuser_pwd = env::var("SUPERUSER_PWD").unwrap_or_else(|_| "password".to_string());

    println!("DB_PORT: {db_port}");
    println!("APP_USER: {app_user}");
    println!("APP_DB_NAME: {app_db_name}");

    // Check that sqlx CLI is installed
    let Ok(_) = Command::new("sqlx").arg("--version").output() else {
        eprintln!("Error: sqlx is not installed.");
        eprintln!("Use:");
        eprintln!(
            "    cargo install --version='~0.8' sqlx-cli --no-default-features --features rustls,postgres"
        );
        eprintln!("to install it.");
        std::process::exit(1);
    };

    let skip_docker = env::var("SKIP_DOCKER").is_ok();

    if !skip_docker {
        // Check if a Postgres container is already running
        let running = Command::new("docker")
            .args(["ps", "--filter", "name=postgres", "--format", "{{.ID}}"])
            .output()
            .expect("failed to run docker ps");

        let container_id = String::from_utf8_lossy(&running.stdout);
        let container_id = container_id.trim();

        if !container_id.is_empty() {
            eprintln!("there is a postgres container already running, kill it with");
            eprintln!("     docker kill {container_id}");
            std::process::exit(1);
        }

        let container_name = format!("postgres_{}", unix_timestamp());

        // Launch Postgres via Docker (detached)
        let status = Command::new("docker")
            .args([
                "run",
                "--env",
                &format!("POSTGRES_USER={superuser}"),
                "--env",
                &format!("POSTGRES_PASSWORD={superuser_pwd}"),
                "--health-cmd",
                "pg_isready -U postgres || exit 1",
                "--health-interval",
                "1s",
                "--health-timeout",
                "5s",
                "--health-retries",
                "5",
                "--publish",
                &format!("{db_port}:5432"),
                "--detach",
                "--name",
                &container_name,
                "postgres",
                "-N",
                "1000",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status()
            .expect("failed to launch postgres container");

        if !status.success() {
            eprintln!("docker run failed with status: {status}");
            std::process::exit(1);
        }

        // Wait until Postgres is healthy
        loop {
            let health = Command::new("docker")
                .args(["inspect", "-f", "{{.State.Health.Status}}", &container_name])
                .output()
                .expect("failed to inspect container");

            let status = String::from_utf8_lossy(&health.stdout);
            let status = status.trim();

            if status == "healthy" {
                break;
            }

            eprintln!("Postgres is still unavailable - sleeping");
            thread::sleep(Duration::from_secs(1));
        }

        // Create the application user
        let create_query = format!("CREATE USER {app_user} WITH PASSWORD '{app_user_pwd}';");
        run_docker_exec(&superuser, &container_name, &create_query);

        // Grant create db privileges
        let grant_query = format!("ALTER USER {app_user} CREATEDB;");
        run_docker_exec(&superuser, &container_name, &grant_query);
    }
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_secs()
}

fn run_docker_exec(superuser: &str, container_name: &str, query: &str) {
    let status = Command::new("docker")
        .args([
            "exec",
            "-i", // Note: -i not -it (no TTY in Rust)
            container_name,
            "psql",
            "-U",
            superuser,
            "-c",
            query,
        ])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("failed to execute psql in container");

    if !status.success() {
        eprintln!("psql query failed: {query}");
        std::process::exit(1);
    }
}
