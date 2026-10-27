# Bundled GeoIP database

`dbip-country-lite-2026-10.mmdb` is the DB-IP "IP to Country Lite" database, release month
**2026-10**. `src/geo.rs` embeds it with `include_bytes!` and reads only `country.iso_code`.
Nothing is downloaded at runtime.

| | |
|---|---|
| Source URL | https://download.db-ip.com/free/dbip-country-lite-2026-10.mmdb.gz |
| Release month | 2026-10 |
| Downloaded | 2026-10-27 |
| `.mmdb.gz` (as downloaded, 4,123,384 bytes) SHA-256 | `4cd44536ddfd40fec7eca94a233f7d19f38d470e6522e653cb8d27001f5fe177` |
| `.mmdb` (decompressed, committed here, 8,348,509 bytes) SHA-256 | `dbd70ccfa2a13627eaf4913d19920a1a229f1b3c5439ac01c509995b6364202a` |
| License | CC BY 4.0, see `DBIP-LICENSE.txt` |

The file is unmodified: it was only decompressed with `gunzip`. DB-IP download links expire
after about two months, so the file is kept in the repository.

## Updating

1. Download the newer `dbip-country-lite-YYYY-MM.mmdb.gz` from https://db-ip.com/db/download/ip-to-country-lite.
2. Decompress it, replace the `.mmdb` here, and update the file name in `src/geo.rs` and `DBIP-LICENSE.txt`.
3. Record the new URL, month and both SHA-256 values in this file.
4. Run `cargo test geo`. `every_database_country_has_a_name` fails if the new data uses a
   country code that the name table in `src/geo.rs` lacks.
