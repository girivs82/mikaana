use crate::DbPool;

pub fn run_migrations(pool: &DbPool) -> Result<(), Box<dyn std::error::Error>> {
    let conn = pool.get()?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS users (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            github_id   INTEGER UNIQUE NOT NULL,
            username    TEXT NOT NULL,
            avatar_url  TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS comments (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            post_slug   TEXT NOT NULL,
            user_id     INTEGER NOT NULL REFERENCES users(id),
            body        TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_comments_slug ON comments(post_slug);

        CREATE TABLE IF NOT EXISTS votes (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id     INTEGER NOT NULL REFERENCES users(id),
            target_type TEXT NOT NULL,
            target_id   INTEGER NOT NULL,
            value       INTEGER NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(user_id, target_type, target_id)
        );
        CREATE INDEX IF NOT EXISTS idx_votes_target ON votes(target_type, target_id);

        CREATE TABLE IF NOT EXISTS categories (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,
            slug        TEXT UNIQUE NOT NULL,
            description TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS threads (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            category_id INTEGER NOT NULL REFERENCES categories(id),
            user_id     INTEGER NOT NULL REFERENCES users(id),
            title       TEXT NOT NULL,
            body        TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_threads_cat ON threads(category_id);

        CREATE TABLE IF NOT EXISTS replies (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            thread_id   INTEGER NOT NULL REFERENCES threads(id),
            user_id     INTEGER NOT NULL REFERENCES users(id),
            body        TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_replies_thread ON replies(thread_id);

        -- Seed default categories if empty
        INSERT OR IGNORE INTO categories (id, name, slug, description) VALUES
            (1, 'General',  'general',  'General discussion'),
            (2, 'Projects', 'projects', 'Discuss projects and ideas'),
            (3, 'Help',     'help',     'Ask for help or advice');
        ",
    )?;

    Ok(())
}
