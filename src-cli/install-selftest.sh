#!/usr/bin/env bash
# Selftest for src-cli/install.sh. Sources the installer with HOME, curl,
# hdiutil, ditto and the browser openers stubbed, then asserts on the pure
# helpers, the frozen parity bytes and the end-to-end flows that stay inside
# the stubbed HOME. The macOS /Applications writing path is not exercised.
# Run: bash src-cli/install-selftest.sh
set -euo pipefail
cd "$(dirname "$0")/.."

ASSERTS=0
assert_eq() {
    local desc=$1 actual=$2 expected=$3
    if [[ $actual == "$expected" ]]; then
        ASSERTS=$((ASSERTS + 1))
    else
        printf 'FAIL %s\n  actual  : %s\n  expected: %s\n' "$desc" "$actual" "$expected" >&2
        exit 1
    fi
}
assert_match() {
    local desc=$1 actual=$2 pattern=$3
    if [[ $actual =~ $pattern ]]; then
        ASSERTS=$((ASSERTS + 1))
    else
        printf 'FAIL %s\n  actual: %s\n  want match: %s\n' "$desc" "$actual" "$pattern" >&2
        exit 1
    fi
}
assert_fail() {
    local desc=$1
    shift
    if ( "$@" ) 2> /dev/null; then
        printf 'FAIL %s: expected a nonzero exit\n' "$desc" >&2
        exit 1
    fi
    ASSERTS=$((ASSERTS + 1))
}

ST_HOME=$(mktemp -d)
ST_WORK=$(mktemp -d)
ST_CURL_LOG="$ST_WORK/curl.log"
ST_RELEASE_FILE="$ST_WORK/release.json"
ST_KEYS_FILE="$ST_WORK/keys.json"
ST_KEYS_FILE2="$ST_WORK/keys2.json"
ST_CREATED="$ST_WORK/created"
export HOME="$ST_HOME"
export XDG_DATA_HOME="$ST_HOME/.local/share"
export XDG_CONFIG_HOME="$ST_HOME/.config"

RELEASE_VERSION="1.2.3"
DMG_ASSET="openwork-mac-arm64-$RELEASE_VERSION.dmg"
APPIMAGE_ASSET="openwork-linux-x86_64-$RELEASE_VERSION.AppImage"
FAKE_BYTES="fake-binary12345"
DMG_SHA_B64=$(printf '%s' "$FAKE_BYTES" | openssl dgst -sha512 -binary | openssl base64 -A)
FAKE_SHA_HEX=$(printf '%s' "$FAKE_BYTES" | openssl dgst -sha512 | awk '{print $NF}')

cat > "$ST_RELEASE_FILE" <<EOF
{"tag_name":"v$RELEASE_VERSION","assets":[
 {"name":"$DMG_ASSET","browser_download_url":"https://example.test/$DMG_ASSET"},
 {"name":"$APPIMAGE_ASSET","browser_download_url":"https://example.test/$APPIMAGE_ASSET"},
 {"name":"latest-mac.yml","browser_download_url":"https://example.test/latest-mac.yml"},
 {"name":"latest-linux.yml","browser_download_url":"https://example.test/latest-linux.yml"}
]}
EOF
cat > "$ST_KEYS_FILE" <<'EOF'
[{"id":"keys_2","name":"RCP Key","is_enabled":1},{"id":"keys_3","name":"Secondary","is_enabled":1},{"id":"keys_4","name":"Disabled","is_enabled":0},{"id":"keys_5","name":"Pending","is_enabled":1,"is_pending":1},{"id":"keys_6","name":"AppKey","is_enabled":1,"is_application":1},{"id":"keys_7","name":"Old","is_enabled":1,"is_legacy":1},{"id":"keys_8","name":"Premium Key","is_enabled":1,"pool":"PREMIUM"}]
EOF
cat > "$ST_KEYS_FILE2" <<'EOF'
[{"id":"keys_2","name":"RCP Key","is_enabled":1},{"id":"keys_3","name":"Secondary","is_enabled":1},{"id":"keys_4","name":"Disabled","is_enabled":0},{"id":"keys_5","name":"Pending","is_enabled":1,"is_pending":1},{"id":"keys_6","name":"AppKey","is_enabled":1,"is_application":1},{"id":"keys_7","name":"Old","is_enabled":1,"is_legacy":1},{"id":"keys_8","name":"Premium Key","is_enabled":1,"pool":"PREMIUM"},{"id":"keys_9","name":"OpenWork","is_enabled":1}]
EOF
printf -- '- url: %s\n  sha512: %s\n' "$APPIMAGE_ASSET" "$DMG_SHA_B64" > "$ST_WORK/latest-linux.yml"

openwork_mock_curl() {
    local -a args=("$@")
    local i url="" out="/dev/null" w_fmt="" a
    for ((i = 0; i < ${#args[@]}; i++)); do
        a=${args[i]}
        case $a in
            -o) out=${args[i + 1]}; ((i += 1)) ;;
            -w) w_fmt=${args[i + 1]}; ((i += 1)) ;;
            http* | example.test*) url=$a ;;
        esac
    done
    printf '%s\n' "$*" >> "$ST_CURL_LOG"
    local w_out="" body=""
    case $url in
        *.dmg | *.AppImage) body=$FAKE_BYTES ;;
        *latest-mac.yml) body=$(printf -- '- url: %s\n  sha512: %s\n' "$DMG_ASSET" "$DMG_SHA_B64") ;;
        *latest-linux.yml) body=$(cat "$ST_WORK/latest-linux.yml") ;;
        *"$OPENWORK_RELEASE_API"*) body=$(cat "$ST_RELEASE_FILE") ;;
        *"/aiaas/api/v1/keys")
            if [[ -s $ST_CREATED ]]; then body=$(cat "$ST_KEYS_FILE2"); else body=$(cat "$ST_KEYS_FILE"); fi
            ;;
        *"$PORTAL_KEYS_PAGE") body="" ;;
        *"$PORTAL_NEW_KEY_MODAL") body='<html><form method="post" action="/aiaas/create_key"><input type="hidden" name="csrfmiddlewaretoken" value="csrfTOKEN123"><select name="unit"><option value="">---------</option><option value="lab1" selected>Lab One</option><option value="lab2">Lab Two</option></select></form></html>' ;;
        *"/aiaas/create_key")
            body='created'
            printf '1' > "$ST_CREATED"
            ;;
        *"/aiaas/showsecret") body='<code>sk-aaaa1111bbbb2222cccc3333</code>' ;;
        *badmeta*) body=$(printf -- '- url: %s\n  sha512: AAAA\n' "$DMG_ASSET") ;;
        *) body="" ;;
    esac
    if [[ $out != /dev/null ]]; then
        printf '%s' "$body" > "$out"
    else
        printf '%s' "$body"
    fi
    if [[ -n $w_fmt ]]; then
        case $url in
            *"/aiaas/create_key") w_out="200 $PORTAL_BASE/aiaas/create_key" ;;
            *) w_out="200 $url" ;;
        esac
        printf '%s' "$w_out"
    fi
}
curl() { openwork_mock_curl "$@"; }
open() { :; }
xdg-open() { :; }
run_with_timeout() { shift; "$@"; }
timeout() { shift; "$@"; }
hdiutil() {
    local -a args=("$@") mp="" i
    for ((i = 1; i < ${#args[@]}; i++)); do
        if [[ ${args[i]} == "-mountpoint" ]]; then mp=${args[i + 1]}; fi
    done
    case $1 in
        attach)
            mkdir -p "$mp/OpenWork.app/Contents/MacOS"
            printf 'macbin' > "$mp/OpenWork.app/Contents/MacOS/OpenWork"
            ;;
        detach) rm -rf "$mp" ;;
    esac
    return 0
}
ditto() { mkdir -p "$2"; cp -R "$1/." "$2/"; }

# shellcheck disable=SC1091
source src-cli/install.sh

openwork_selftest_reset() {
    rm -rf "$ST_HOME/.config"
    mkdir -p "$ST_HOME/.config/openwork" "$ST_HOME/Downloads"
    CONFDIR="$ST_HOME/.config/openwork"
    # fixture globals for the stubbed environment, read by the installer at runtime
    # shellcheck disable=SC2034
    LOG_FILE="$CONFDIR/install.log"
    # shellcheck disable=SC2034
    SESSION_HEADER="" SESSION_CSRF="" MANUAL_TOKEN="" API_TOKEN="" REVEAL_KEY_ID=""
    # shellcheck disable=SC2034
    FORM_URL="" MODAL_CSRF="" UNITS_TSV=""
    : > "$ST_CURL_LOG"
    rm -f "$ST_CREATED"
}
openwork_selftest_reset

# 1. json helpers round-trip
out=$(json_canonical '{"b":1,"a":{"z":true,"y":[2,1]},"c":null}')
assert_eq "json_canonical sorts recursively" "$out" '{"a":{"y":[2,1],"z":true},"b":1,"c":null}'
out=$(json_canonical '[]')
assert_eq "json_canonical empty array" "$out" '[]'
out=$(jsonc_strip '{"a":1, /* c */ "b": [1, 2,], }')
assert_eq "jsonc_strip drops comments and trailing commas" "$out" '{"a":1,  "b": [1, 2] }'
assert_eq "jsonc_strip output parses" "$(sqlite3 :memory: "SELECT json_valid('$(sqlq "$out")');")" "1"
out=$(json_pretty '{"a":[1,2],"b":{"c":"x\"y"}}')
PRETTY_EXPECT=$'{\n  "a": [\n    1,\n    2\n  ],\n  "b": {\n    "c": "x\\"y"\n  }\n}'
assert_eq "json_pretty 2-space shape" "$out" "$PRETTY_EXPECT"
assert_eq "b64_to_hex" "$(b64_to_hex "$DMG_SHA_B64")" "$FAKE_SHA_HEX"
assert_fail "b64_to_hex rejects junk" b64_to_hex "not-base64!!"

# 2. frozen parity bytes: definition fingerprints to the pinned ownership hash
FP=$(DEFINITION="$DEFINITION_JSON" python3 -c '
import hashlib, json, os, sys
value = json.loads(os.environ["DEFINITION"])
def canon(v):
    if isinstance(v, dict):
        return "{" + ",".join(json.dumps(k) + ":" + canon(x) for k, x in sorted(v.items())) + "}"
    if isinstance(v, list):
        return "[" + ",".join(canon(x) for x in v) + "]"
    return json.dumps(v)
sys.stdout.write(canon(value))
')
assert_eq "definition canonical bytes equal the input" "$FP" "$DEFINITION_JSON"
HASH=$(printf '%s' "$FP" | sha256sum | awk '{print $1}')
assert_eq "definition sha256 equals the machine ownership fingerprint" "$HASH" "$MACHINE_OWNERSHIP_FP"
assert_eq "engine global frozen bytes" "$ENGINE_GLOBAL_JSON" "{\"disabled_providers\":[\"openwork-free\"],\"provider\":{\"epfl-aiaas\":$DEFINITION_JSON}}"
assert_eq "default model row frozen bytes" "$MODEL_ROW_JSON" '{"modelID":"epfl-aiaas/large","providerID":"epfl-aiaas"}'

# 3. merge over an empty base: schema first, providers nested, token in place
out=$(merge_oc_config '{}' 'tok')
assert_eq "fresh merge shape" "$out" '{"$schema":"https://opencode.ai/config.json","providers":{"epfl-rcp-aiaas":'"$(oc_provider_block 'tok')"'}}'

# 4. merge over an existing base preserves insertion order and replaces wholesale
out=$(merge_oc_config '{"model":"x","theme":"dark","providers":{"other":{}}}' 'tok2')
assert_match "merge keeps leading base keys" "$out" '^\{"\$schema":"https://opencode.ai/config.json","model":"x","theme":"dark","providers":\{"other":\{\},"epfl-rcp-aiaas":'
assert_match "merge replaces the rcp provider" "$out" '"apiKey":"tok2"'

# 5. oc_provider_block content is byte-identical to the frozen provider definition
BLOCK=$(oc_provider_block '{env:EPFL_AIAAS_API_KEY}')
EXPECTED_BLOCK=$(printf '{"name":"RCP AI Inference as a Service","package":"aisdk:@ai-sdk/openai-compatible","settings":{"baseURL":"%s","apiKey":"{env:EPFL_AIAAS_API_KEY}"},"models":{"epfl-aiaas/large":{"name":"RCP AIaaS Large","modelID":"epfl-aiaas/large","capabilities":{"input":["image","text"],"output":["text"]},"cost":{"input":0.1664,"output":0.4991,"cache":{"read":0.0166}},"settings":{}},"epfl-aiaas/small":{"name":"RCP AIaaS Small","modelID":"epfl-aiaas/small","capabilities":{"input":["image","text"],"output":["text"]},"cost":{"input":0.1671,"output":0.5014,"cache":{"read":0.0167}},"settings":{"reasoningEffort":"none"},"variants":[{"id":"none","settings":{"reasoningEffort":"none"}},{"id":"low","settings":{"reasoningEffort":"low"}},{"id":"high","settings":{"reasoningEffort":"high"}},{"id":"xhigh","settings":{"reasoningEffort":"xhigh"}},{"id":"max","settings":{"reasoningEffort":"max"}}]}}}' "$RCP_BASE_URL")
assert_eq "provider block bytes" "$BLOCK" "$EXPECTED_BLOCK"

# 6. write_opencode_config writes the JSON.stringify shape, then is idempotent
write_opencode_config 'sk-tok'
FILE="$ST_HOME/.config/opencode/opencode.json"
assert_match "config written with schema line first" "$(head -1 "$FILE")" '^\{$'
assert_match "config holds the token" "$(grep -c '"apiKey": "sk-tok"' "$FILE")" '1'
MT1=$(stat -c %Y "$FILE")
write_opencode_config 'sk-tok'
assert_eq "second write is a no-op" "$(stat -c %Y "$FILE")" "$MT1"
write_opencode_config 'sk-tok-2'
assert_match "token rotated in place" "$(grep -c '"apiKey": "sk-tok-2"' "$FILE")" '1'

# 7. key list filtering: pending, disabled, legacy and application keys drop out
TSV=$(keys_selectable_tsv "$(cat "$ST_KEYS_FILE")")
assert_eq "selectable key count" "$(printf '%s\n' "$TSV" | grep -c . || true)" "3"
assert_eq "selectable keys sorted by name" "$(printf '%s\n' "$TSV" | head -1 | cut -f2)" "Premium Key"

# 8. session extraction helpers
assert_eq "sessionid extraction" "$(extract_pasted_session 'foo; sessionid=abc123; csrftoken=xyz')" "abc123"
assert_eq "sessionid bare value" "$(extract_pasted_session 'abc123def456ghi789jkl012mno345pq')" "abc123def456ghi789jkl012mno345pq"
assert_eq "short junk not a sessionid" "$(extract_pasted_session 'abc123' || true)" ""

# 9. saved portal session is probed and reused
printf 'sessionid=GOODSESS' > "$CONFDIR/portal-session"
portal_login < /dev/null
assert_eq "saved session reused" "$SESSION_HEADER" "sessionid=GOODSESS"

# 10. pasted key secret during the poll window becomes the manual token
openwork_selftest_reset
portal_login < <(printf 'sk-aaaa1111bbbb2222cccc3333\n')
assert_eq "pasted sk- token captured" "$MANUAL_TOKEN" "sk-aaaa1111bbbb2222cccc3333"

# 11. rcp_key_step numeric pick flows through reveal_secret
openwork_selftest_reset
SESSION_HEADER="sessionid=STUBSESS"
rcp_key_step <<< "1"
assert_eq "numeric pick sets the token" "$API_TOKEN" "sk-aaaa1111bbbb2222cccc3333"
assert_match "reveal posted to the portal" "$(grep -c 'aiaas/showsecret' "$ST_CURL_LOG")" '[1-9]'
assert_match "reveal sent the session cookie" "$(grep -c 'Cookie: sessionid=STUBSESS' "$ST_CURL_LOG")" '[1-9]'

# 12. create_key_flow: modal parse, unit pick, name pick, POST, refreshed list
openwork_selftest_reset
SESSION_HEADER="sessionid=STUBSESS"
create_key_flow "$(cat "$ST_KEYS_FILE")" <<< $'1\nOpenWork\n'
assert_eq "created key id from the refreshed list" "$REVEAL_KEY_ID" "keys_9"
assert_match "create POST submitted" "$(grep -c 'aiaas/create_key' "$ST_CURL_LOG")" '[1-9]'
assert_match "create POST carried csrf and pool" "$(grep -c 'pool=BASIC' "$ST_CURL_LOG")" '[1-9]'

# 13. runtime rows: fresh write, values, then idempotence
openwork_selftest_reset
write_runtime_rows
DB="$CONFDIR/runtime.sqlite"
assert_eq "engine global row" "$(read_config_row "$DB" runtime_opencode_configs "$ENGINE_GLOBAL_ID" config_json)" "$ENGINE_GLOBAL_JSON"
assert_eq "ownership row" "$(read_config_row "$DB" openwork_workspace_configs "$MACHINE_OWNERSHIP_ID" config_json)" '{"epfl-aiaas":"'"$MACHINE_OWNERSHIP_FP"'"}'
BEFORE=$(read_config_row "$DB" runtime_opencode_configs "$ENGINE_GLOBAL_ID" config_json)
write_runtime_rows
assert_eq "engine global row idempotent" "$(read_config_row "$DB" runtime_opencode_configs "$ENGINE_GLOBAL_ID" config_json)" "$BEFORE"

# 14. merge path: a modified existing engine global row is upgraded and canonicalized
openwork_selftest_reset
write_runtime_rows
sqlite3 "$DB" "UPDATE runtime_opencode_configs SET config_json = '{\"provider\":{\"other\":{}},\"extra\":1}' WHERE workspace_id = '$ENGINE_GLOBAL_ID';"
write_runtime_rows
MERGED=$(read_config_row "$DB" runtime_opencode_configs "$ENGINE_GLOBAL_ID" config_json)
assert_eq "merged row canonicalized with definition and extra key" "$MERGED" '{"disabled_providers":["openwork-free"],"extra":1,"provider":{"epfl-aiaas":'"$DEFINITION_JSON"',"other":{}}}'
assert_match "other provider kept" "$MERGED" '"other":\{\}'

# 15. engine global row corruption is a hard error
openwork_selftest_reset
write_runtime_rows
sqlite3 "$DB" "UPDATE runtime_opencode_configs SET config_json = 'not json' WHERE workspace_id = '$ENGINE_GLOBAL_ID';"
assert_fail "corrupt engine global row aborts" write_runtime_rows

# 16. workspace default model upsert
openwork_selftest_reset
write_runtime_rows
openwork_gui_upsert_row() { upsert_row "$CONFDIR/runtime.sqlite" "$@"; }
openwork_gui_upsert_row workspace_default_models model_json "__selftest_ws__" "$MODEL_ROW_JSON" "$(now_ms)"
assert_eq "default model row stored" "$(read_config_row "$CONFDIR/runtime.sqlite" workspace_default_models "__selftest_ws__" model_json)" "$MODEL_ROW_JSON"

# 17. env store: fresh write, canonical sorted array, idempotence, rotation
openwork_selftest_reset
write_env_store 'sk-tok'
ENVFILE="$CONFDIR/env.json"
assert_match "env store holds the key" "$(grep -c '"key": "EPFL_AIAAS_API_KEY"' "$ENVFILE")" '1'
assert_match "env store masked token" "$(grep -c '"value": "sk-tok"' "$ENVFILE")" '1'
write_env_store 'sk-tok'
MT2=$(stat -c %Y "$ENVFILE")
write_env_store 'sk-tok'
assert_eq "env store idempotent" "$(stat -c %Y "$ENVFILE")" "$MT2"
write_env_store 'sk-tok-2'
assert_match "env store rotated" "$(grep -c '"value": "sk-tok-2"' "$ENVFILE")" '1'

# 18. env store with a foreign variable keeps it sorted before ours
openwork_selftest_reset
printf '{"schemaVersion":1,"updatedAt":1,"variables":[{"key":"ZZZ_OLD","updatedAt":1,"value":"v"}]}' > "$CONFDIR/env.json"
write_env_store 'sk-tok'
ORDER=$(sqlite3 :memory: "SELECT group_concat(json_extract(value,'\$.key'), ',') FROM json_each(readfile('$CONFDIR/env.json'),'\$.variables');")
assert_eq "variables sorted by key" "$ORDER" "EPFL_AIAAS_API_KEY,ZZZ_OLD"
assert_match "foreign variable kept" "$(grep -c 'ZZZ_OLD' "$CONFDIR/env.json")" '1'

# 19. checksum verify: wrong hash dies, right hash passes
printf '%s' "$FAKE_BYTES" > "$ST_WORK/blob.bin"
verify_sha512 "$ST_WORK/blob.bin" "$APPIMAGE_ASSET" "https://example.test/latest-linux.yml"
assert_eq "checksum verify ok" "$?" "0"
assert_fail "checksum mismatch dies" verify_sha512 "$ST_WORK/blob.bin" "$DMG_ASSET" "https://example.test/badmeta"

# 20. linux AppImage install end to end inside the stubbed HOME
openwork_selftest_reset
# shellcheck disable=SC2034
OS="linux"
# shellcheck disable=SC2034
ARCH="x86_64"
install_openwork_linux
APPIMG="$ST_HOME/.local/share/openwork/OpenWork.AppImage"
assert_eq "AppImage bytes" "$(cat "$APPIMG")" "$FAKE_BYTES"
assert_eq "version stamp" "$(cat "$ST_HOME/.local/share/openwork/OpenWork.version")" "$RELEASE_VERSION"
assert_match "desktop entry" "$(grep -c 'Exec="' "$ST_HOME/.local/share/applications/openwork.desktop")" '1'

# 21. foreign opencode state database is backed up
openwork_selftest_reset
# shellcheck disable=SC2034
OS="linux"
STATEDIR="$ST_HOME/.local/share/opencode"
mkdir -p "$STATEDIR"
sqlite3 "$STATEDIR/foreign.db" "CREATE TABLE other (x);"
repair_opencode_state
FOREIGN_BAK=$(find "$STATEDIR" -maxdepth 1 -name 'foreign.db.bak-*' | wc -l)
assert_eq "foreign db renamed" "$FOREIGN_BAK" "1"
sqlite3 "$STATEDIR/session.db" "CREATE TABLE session (x);"
repair_opencode_state
assert_eq "opencode session db kept" "$(find "$STATEDIR" -maxdepth 1 -name 'session.db' | wc -l)" "1"

# 22. workspace heal: removes the stale provider, deletes a schema-only file
openwork_selftest_reset
WSDIR="$ST_HOME/ws"
mkdir -p "$WSDIR"
printf '{"$schema":"https://opencode.ai/config.json","model":"x","provider":{"other":{},"epfl-aiaas":{"settings":{}}}}' > "$WSDIR/opencode.jsonc"
heal_workspace_config "$WSDIR"
assert_match "stale provider removed" "$(grep -c 'epfl-aiaas' "$WSDIR/opencode.jsonc" || true)" '0'
assert_match "other provider kept" "$(grep -c '"other"' "$WSDIR/opencode.jsonc")" '1'
printf '{"$schema":"https://opencode.ai/config.json","provider":{"epfl-aiaas":{"settings":{}}}}' > "$WSDIR/opencode.jsonc"
heal_workspace_config "$WSDIR"
assert_eq "schema-only file deleted" "$([[ -f $WSDIR/opencode.jsonc ]] && echo present || echo gone)" "gone"
printf '// note\n{"provider":{"epfl-aiaas":{}}}' > "$WSDIR/opencode.jsonc"
heal_workspace_config "$WSDIR"
assert_match "commented file untouched" "$(grep -c '// note' "$WSDIR/opencode.jsonc")" '1'

# 23. telemetry disable
openwork_selftest_reset
disable_telemetry
assert_eq "telemetry disabled" "$(cat "$CONFDIR/free-auto-analytics")" "disabled"
[[ ! -e $CONFDIR/free-auto-journey ]]
ASSERTS=$((ASSERTS + 1))

printf 'selftest: %s checks passed\n' "$ASSERTS"
