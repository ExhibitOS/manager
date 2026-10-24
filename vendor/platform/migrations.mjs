// SPDX-License-Identifier: AGPL-3.0-or-later
import { readFile, readdir } from "node:fs/promises";
import { createHash } from "node:crypto";
/** Apply the migration sequence inside a transaction owned by the caller. */
export async function migrateInTransaction(client, directory, through) {
    await client.query("SELECT pg_advisory_xact_lock(82001)");
    await client.query("CREATE TABLE IF NOT EXISTS schema_migrations (name text PRIMARY KEY, sha256 text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())");
    for (const name of (await readdir(directory))
        .filter((x) => /^\d+.*\.sql$/.test(x))
        .sort()) {
        if (through && name > through)
            continue;
        const sql = await readFile(`${directory}/${name}`);
        const hash = createHash("sha256").update(sql).digest("hex");
        const existing = await client.query("SELECT sha256 FROM schema_migrations WHERE name=$1", [name]);
        if (existing.rowCount) {
            if (existing.rows[0].sha256 !== hash)
                throw new Error("migration checksum mismatch");
            continue;
        }
        await client.query(sql.toString());
        await client.query("INSERT INTO schema_migrations(name,sha256) VALUES($1,$2)", [name, hash]);
    }
}
