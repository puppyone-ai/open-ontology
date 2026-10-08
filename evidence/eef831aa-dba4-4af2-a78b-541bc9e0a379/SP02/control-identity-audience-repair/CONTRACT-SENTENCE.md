# Corrected ID Token contract sentence

Written before any edit under `tools/control-identity`. Pre-edit `CONTRACT.md` sha256 is `35f9c4a1fccf08b695848c6e6291f191a392cf8ecd079490f80855a773035732`, the same bytes as contract commit `089b00d7f70da1bc42f7e201dfa070748c651d5b`. Product commit under review is `35bd6b8157cecd89f2ae085b012b4b0137562ddb`.

The frozen row says: `iss` exact, `aud` contains the configured audience, multiple audiences require `azp` equal to `client_id`, a present `azp` equals `client_id`.

OIDC Core 1.0 §3.1.3.7 requires the ID Token `aud` claim to contain the relying party's registered `client_id` for every configuration the file format permits. A distinct legacy configured audience does not replace `client_id`.

Exact corrected sentence, which replaces that ID Token row:

`iss` exact, `aud` contains the registered `client_id` for every permitted configuration, a distinct configured audience does not replace `client_id`, multiple audiences require `azp` equal to `client_id`, a present `azp` equals `client_id`, and a single audience equal to `client_id` is accepted when `azp` is absent

Wire compatibility that stays in place: `schema/config.schema.json` keeps `audience` required, length 1..=128, and the `config_commit` preimage still includes `audience`. OpenAPI operations, error code `audience.rejected`, and the response schemas stay byte-stable. The configured `audience` string is not the ID Token audience check.

Rejection code for a failed `aud` or `azp` check remains `audience.rejected`.
