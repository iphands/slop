use qctrl_rcon::RconClient;
use std::time::Instant;

#[tokio::main]
async fn main() {
    let test_cases = vec![
        ("noir.lan", 27910, "ace123"),
        ("192.168.10.10", 27910, "ace123"),
    ];

    for (host, port, password) in test_cases {
        println!("\n=== Testing RCON to {}:{} ===", host, port);
        let client = RconClient::new(host, port, password);
        
        let start = Instant::now();
        match client.execute("status").await {
            Ok(output) => {
                let duration = start.elapsed();
                println!("✓ SUCCESS ({}ms)", duration.as_millis());
                println!("Response length: {} bytes", output.len());
                if output.len() > 200 {
                    println!("Preview: {}...", &output[..200]);
                } else {
                    println!("Response:\n{}", output);
                }
            }
            Err(e) => {
                let duration = start.elapsed();
                println!("✗ FAILED ({}ms)", duration.as_millis());
                println!("Error: {}", e);
            }
        }
    }

    println!("\n=== Testing different commands to 192.168.10.10 ===");
    let client = RconClient::new("192.168.10.10", 27910, "ace123");
    
    let commands = vec!["status", "dmflags", "map q2dm1"];
    for cmd in commands {
        println!("\n--- Command: {} ---", cmd);
        let start = Instant::now();
        match client.execute(cmd).await {
            Ok(output) => {
                let duration = start.elapsed();
                println!("✓ SUCCESS ({}ms)", duration.as_millis());
                println!("Output: {}", output.lines().take(5).collect::<Vec<_>>().join("\n"));
                if output.lines().count() > 5 {
                    println!("... ({} more lines)", output.lines().count() - 5);
                }
            }
            Err(e) => {
                let duration = start.elapsed();
                println!("✗ FAILED ({}ms)", duration.as_millis());
                println!("Error: {}", e);
            }
        }
    }
}
