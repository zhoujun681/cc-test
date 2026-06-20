use rusqlite::Connection;
use std::path::Path;

pub fn dump_database(db_path: &str) -> Result<(), String> {
    if !Path::new(db_path).exists() {
        return Err(format!("Database file not found: {}", db_path));
    }

    let conn = Connection::open(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    println!("\n=== Database Structure ===");
    
    // List all tables
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .map_err(|e| format!("Failed to query tables: {}", e))?;
    
    let tables: Vec<String> = stmt.query_map([], |row| row.get(0))
        .map_err(|e| format!("Failed to get tables: {}", e))?
        .filter_map(|r| r.ok())
        .collect();
    
    println!("Tables found: {:?}", tables);

    // Dump each table's content
    for table in &tables {
        println!("\n=== Table: {} ===", table);
        
        // Get column info
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))
            .map_err(|e| format!("Failed to get table info: {}", e))?;
        
        let columns: Vec<String> = stmt.query_map([], |row| {
            let name: String = row.get(1)?;
            Ok(name)
        })
        .map_err(|e| format!("Failed to get columns: {}", e))?
        .filter_map(|r| r.ok())
        .collect();
        
        println!("Columns: {:?}", columns);
        
        // Count rows
        let count: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM {}", table),
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        
        println!("Row count: {}", count);
        
        // Show first few rows
        if count > 0 {
            let query = format!("SELECT * FROM {} LIMIT 5", table);
            if let Ok(mut stmt) = conn.prepare(&query) {
                let column_count = columns.len();
                let rows = stmt.query_map([], |row| {
                    let mut values = Vec::new();
                    for i in 0..column_count {
                        let val: String = row.get::<_, Option<String>>(i)
                            .unwrap_or(None)
                            .unwrap_or_else(|| "<NULL>".to_string());
                        values.push(val);
                    }
                    Ok(values)
                });
                
                if let Ok(rows) = rows {
                    for (idx, row) in rows.enumerate() {
                        if let Ok(values) = row {
                            println!("Row {}: {:?}", idx + 1, values);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
