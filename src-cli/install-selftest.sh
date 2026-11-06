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
        *"$PORTAL_NEW_KEY_MODAL") body='<html><form method="post" action="/aiaas/create_key"><input type="hidden" name="csrfmiddlewaretoken" value="csrfTOKEN123"><select name="unit"><option value="">---------</option><option value="lab1" selected>Lab One</option><option value="lab2">Lab Two</option></select><select name="pool"><option value="BASIC">Basic</option><option value="PREMIUM">Premium</option></select><select name="usage"><option value="PERSONAL">Personal</option><option value="APPLICATION">Application</option></select></form></html>' ;;
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
SESSION_CSRF="cookieCSRF"
create_key_flow "$(cat "$ST_KEYS_FILE")" <<< $'1\nOpenWork\n'
assert_eq "created key id from the refreshed list" "$REVEAL_KEY_ID" "keys_9"
assert_match "create POST submitted" "$(grep -c 'aiaas/create_key' "$ST_CURL_LOG")" '[1-9]'
assert_match "create POST carried csrf and pool" "$(grep -c 'pool=BASIC' "$ST_CURL_LOG")" '[1-9]'
assert_match "modal csrf wins over the session cookie" "$(grep -c 'cookieCSRF' "$ST_CURL_LOG")" '^0$'
assert_match "unit picker scoped to the unit select" "$(grep -c 'unit=lab1' "$ST_CURL_LOG")" '[1-9]'

# 12b. parse_modal_form scopes options to the unit select and takes the first form target
openwork_selftest_reset
parse_modal_form '<html><div hx-post="#ignored"></div><form hx-post="/aiaas/create_key"><input type="hidden" name="csrfmiddlewaretoken" value="tok1"><meta name="csrfmiddlewaretoken" content="tok2"><select name="unit"><option value="">---------</option><option value="lab1">Lab One</option></select><select name="pool"><option value="BASIC">Basic</option><option value="PREMIUM">Premium</option></select></form></html>'
assert_eq "first hx-post target" "$FORM_URL" "/aiaas/create_key"
assert_eq "unit options only" "$(printf '%s' "$UNITS_TSV" | cut -f1 | paste -sd,)" "lab1"
assert_eq "hidden csrf wins, meta is a fallback" "$MODAL_CSRF" "tok1"
parse_modal_form '<html><form action="/aiaas/create_key"><meta name="csrfmiddlewaretoken" content="tok2"></form></html>'
assert_eq "action fallback" "$FORM_URL" "/aiaas/create_key"
assert_eq "meta csrf fallback" "$MODAL_CSRF" "tok2"
parse_modal_form '<html><form hx-post="#self" action="/aiaas/create_key"><input type="hidden" name="csrfmiddlewaretoken" value="tok3"></form></html>'
assert_eq "hash target skipped, action wins" "$FORM_URL" "/aiaas/create_key"
assert_eq "input csrf wins over meta" "$MODAL_CSRF" "tok3"
parse_modal_form '<html><div hx-post="/accounts/logout"></div><form id="k"><input type="hidden" name="csrfmiddlewaretoken" value="tok4"></form><button hx-post="/aiaas/modal_key_new"></button></html>'
assert_eq "button and div targets ignored" "$FORM_URL" ""

# 12c. fresh_csrf_from_headers keeps the first Set-Cookie token
openwork_selftest_reset
printf 'Set-Cookie: csrftoken=first; Path=/\nSet-Cookie: csrftoken=second; Path=/\n' > "$ST_WORK/hdrs"
fresh_csrf_from_headers "$ST_WORK/hdrs"
assert_eq "csrf takes the first set-cookie" "$SESSION_CSRF" "first"

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

# 14b. an engine global row without a provider key gains the definition
openwork_selftest_reset
write_runtime_rows
sqlite3 "$DB" "UPDATE runtime_opencode_configs SET config_json = '{\"extra\":1}' WHERE workspace_id = '$ENGINE_GLOBAL_ID';"
write_runtime_rows
MERGED=$(read_config_row "$DB" runtime_opencode_configs "$ENGINE_GLOBAL_ID" config_json)
assert_eq "missing provider map rebuilt with the definition" "$MERGED" '{"disabled_providers":["openwork-free"],"extra":1,"provider":{"epfl-aiaas":'"$DEFINITION_JSON"'}}'

# 14c. a non-object provider map or non-object row is a hard error
openwork_selftest_reset
write_runtime_rows
sqlite3 "$DB" "UPDATE runtime_opencode_configs SET config_json = '{\"provider\":\"nope\"}' WHERE workspace_id = '$ENGINE_GLOBAL_ID';"
assert_fail "non-object provider map aborts" write_runtime_rows
sqlite3 "$DB" "UPDATE runtime_opencode_configs SET config_json = '{\"provider\":null}' WHERE workspace_id = '$ENGINE_GLOBAL_ID';"
assert_fail "null provider map aborts" write_runtime_rows
sqlite3 "$DB" "UPDATE runtime_opencode_configs SET config_json = '[1]' WHERE workspace_id = '$ENGINE_GLOBAL_ID';"
assert_fail "non-object engine row aborts" write_runtime_rows

# 14d. a corrupt or non-object ownership row is a hard error
openwork_selftest_reset
write_runtime_rows
sqlite3 "$DB" "UPDATE openwork_workspace_configs SET config_json = 'not json' WHERE workspace_id = '$MACHINE_OWNERSHIP_ID';"
assert_fail "corrupt ownership row aborts" write_runtime_rows
sqlite3 "$DB" "UPDATE openwork_workspace_configs SET config_json = '[2]' WHERE workspace_id = '$MACHINE_OWNERSHIP_ID';"
assert_fail "non-object ownership row aborts" write_runtime_rows

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

# 17b. a foreign top-level key and a non-1 schemaVersion survive a write
openwork_selftest_reset
printf '{"schemaVersion":7,"updatedAt":1,"variables":[],"channel":"beta"}' > "$CONFDIR/env.json"
write_env_store 'sk-tok'
assert_match "schema version preserved" "$(grep -c '"schemaVersion": 7' "$CONFDIR/env.json")" '1'
assert_match "foreign key preserved" "$(grep -c '"channel": "beta"' "$CONFDIR/env.json")" '1'

# 17c. a non-object env store is a hard error
openwork_selftest_reset
printf '"just a string"' > "$CONFDIR/env.json"
assert_fail "non-object env store aborts" write_env_store sk-tok
printf '[]' > "$CONFDIR/env.json"
assert_fail "array env store aborts" write_env_store sk-tok
printf '{"schemaVersion":1,"updatedAt":1,"variables":"gone"}' > "$CONFDIR/env.json"
assert_fail "non-array variables aborts" write_env_store sk-tok
printf '{"schemaVersion":1,"updatedAt":1,"variables":["bare"]}' > "$CONFDIR/env.json"
assert_fail "non-object variable aborts" write_env_store sk-tok

# 17d. a non-object opencode.json is a hard error
openwork_selftest_reset
mkdir -p "$ST_HOME/.config/opencode"
printf '[1]' > "$ST_HOME/.config/opencode/opencode.json"
assert_fail "non-object opencode config aborts" write_opencode_config sk-tok

# 18. env store with a foreign variable keeps it sorted before ours
openwork_selftest_reset
printf '{"schemaVersion":1,"updatedAt":1,"variables":[{"key":"ZZZ_OLD","updatedAt":1,"value":"v"}]}' > "$CONFDIR/env.json"
write_env_store 'sk-tok'
ORDER=$(sqlite3 :memory: "SELECT group_concat(json_extract(value,'\$.key'), ',') FROM json_each(readfile('$CONFDIR/env.json'),'\$.variables');")
assert_eq "variables sorted by key" "$ORDER" "EPFL_AIAAS_API_KEY,ZZZ_OLD"
assert_match "foreign variable kept" "$(grep -c 'ZZZ_OLD' "$CONFDIR/env.json")" '1'

# 18b. a variable entry without a key survives, like merge_env_store
openwork_selftest_reset
printf '{"schemaVersion":1,"updatedAt":1,"variables":[{"updatedAt":1,"value":"mystery"},{"key":"ZZZ_OLD","updatedAt":1,"value":"v"}]}' > "$CONFDIR/env.json"
write_env_store 'sk-tok'
assert_match "keyless variable kept" "$(grep -c '"value": "mystery"' "$CONFDIR/env.json")" '1'
assert_match "keyed variable kept beside keyless" "$(grep -c 'ZZZ_OLD' "$CONFDIR/env.json")" '1'

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
: > "$STATEDIR/empty.db"
repair_opencode_state
assert_eq "empty db kept like the GUI" "$(find "$STATEDIR" -maxdepth 1 -name 'empty.db' | wc -l)" "1"

# 21b. the macOS legacy state dir is scanned on every OS
openwork_selftest_reset
LEGACY="$ST_HOME/Library/Application Support/opencode"
mkdir -p "$LEGACY"
sqlite3 "$LEGACY/legacy.db" "CREATE TABLE other (x);"
repair_opencode_state
assert_eq "legacy mac dir scanned on linux" "$(find "$LEGACY" -maxdepth 1 -name 'legacy.db.bak-*' | wc -l)" "1"

# 21c. run_with_timeout without timeout(1), stock macOS, passes output through
ST_BIN="$ST_WORK/bin"
mkdir -p "$ST_BIN"
for t in bash mktemp cat rm sleep; do
    p=$(command -v "$t" 2> /dev/null) && ln -sf "$p" "$ST_BIN/$t"
done
OUT=$(PATH="$ST_BIN" bash -c "$(declare -f run_with_timeout)
run_with_timeout 5 bash -c 'echo fallback-output'
")
assert_eq "fallback passes output through" "$OUT" "fallback-output"

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
printf '{"provider":{"epfl-aiaas":{}}}' > "$WSDIR/opencode.jsonc"
heal_workspace_config "$WSDIR"
assert_eq "provider-only file deleted" "$([[ -f $WSDIR/opencode.jsonc ]] && echo present || echo gone)" "gone"

# 22b. a selected id missing from workspaces[] keeps its id, no fallback re-pick
openwork_selftest_reset
WS2="$ST_HOME/ws2"
mkdir -p "$WS2" "$ST_HOME/.config/com.differentai.openwork"
printf '{"selectedId":"ws_selected","workspaces":[{"id":"ws_other","path":"%s","workspaceType":"local"}]}' "$WS2" > "$ST_HOME/.config/com.differentai.openwork/openwork-workspaces.json"
assert_eq "selected id kept when workspaces lost it" "$(resolve_workspace)" "ws_selected"

# 23. telemetry disable
openwork_selftest_reset
disable_telemetry
assert_eq "telemetry disabled" "$(cat "$CONFDIR/free-auto-analytics")" "disabled"
[[ ! -e $CONFDIR/free-auto-journey ]]
ASSERTS=$((ASSERTS + 1))

# 24. payloads over the 128 KiB per-argument exec cap (MAX_ARG_STRLEN on
# Linux) still reach sqlite3: the SQL travels on stdin, never in argv.
openwork_selftest_reset
BIG_KEYS="$ST_WORK/keys-big.json"
python3 - "$BIG_KEYS" <<'PY'
import json, sys
keys = [{"id": "keys_%d" % i, "name": "Key %04d" % i, "is_enabled": 1, "pool": "BASIC", "note": "n" * 300} for i in range(600)]
with open(sys.argv[1], "w") as f:
    f.write(json.dumps(keys, sort_keys=True, separators=(",", ":")))
PY
BIG=$(cat "$BIG_KEYS")
assert_eq "big key list clears the 128 KiB argv cap" "$(( ${#BIG} > 131072 ? 1 : 0 ))" "1"
assert_eq "big key list validates" "$(jvalid "$BIG" && printf 1 || printf 0)" "1"
assert_eq "big key list canonicalizes byte for byte" "$(json_canonical "$BIG")" "$BIG"
assert_eq "big key list stays selectable" "$(keys_selectable_tsv "$BIG" | grep -c . || true)" "600"

# 24b. a big existing env store is merged, every foreign variable kept
openwork_selftest_reset
python3 - "$CONFDIR/env.json" <<'PY'
import json, sys
variables = [{"key": "ZZZ_%04d" % i, "updatedAt": 1, "value": "v" * 300} for i in range(500)]
with open(sys.argv[1], "w") as f:
    f.write(json.dumps({"schemaVersion": 1, "updatedAt": 1, "variables": variables}))
PY
assert_eq "big env store clears the argv cap" "$(( $(wc -c < "$CONFDIR/env.json") > 131072 ? 1 : 0 ))" "1"
write_env_store 'sk-big'
assert_eq "big env store keeps every variable" "$(grep -c '"key":' "$CONFDIR/env.json")" "501"
assert_match "big env store holds the token" "$(grep -c '"value": "sk-big"' "$CONFDIR/env.json")" '1'

# 24c. a big opencode.json is merged, not rejected
openwork_selftest_reset
mkdir -p "$ST_HOME/.config/opencode"
python3 - "$ST_HOME/.config/opencode/opencode.json" <<'PY'
import json, sys
with open(sys.argv[1], "w") as f:
    f.write(json.dumps({"model": "x", "extra": ["e" * 300] * 500}))
PY
write_opencode_config 'sk-big'
assert_match "big opencode config merged" "$(grep -c 'epfl-rcp-aiaas' "$ST_HOME/.config/opencode/opencode.json")" '1'
assert_match "big opencode config kept its keys" "$(grep -c '"model": "x"' "$ST_HOME/.config/opencode/opencode.json")" '1'

# 24d. a big row value is written and read back
openwork_selftest_reset
write_runtime_rows
BIGVAL=$(printf '{"blob":"%s"}' "$(head -c 140000 /dev/zero | tr '\0' 'a')")
upsert_row "$CONFDIR/runtime.sqlite" openwork_workspace_configs config_json big_ws "$BIGVAL" 1
assert_eq "big row value round-trips" "$(read_config_row "$CONFDIR/runtime.sqlite" openwork_workspace_configs big_ws config_json)" "$BIGVAL"

# 24e. the reported failure: the key step over a big portal key list
openwork_selftest_reset
SESSION_HEADER="sessionid=STUBSESS"
python3 - "$ST_WORK/keys-big2.json" <<'PY'
import json, sys
keys = [{"id": "keys_%d" % i, "name": "Key %04d" % i, "is_enabled": 1, "pool": "BASIC", "note": "n" * 300} for i in range(600)]
keys.append({"id": "keys_openwork", "name": "OpenWork", "is_enabled": 1})
with open(sys.argv[1], "w") as f:
    f.write(json.dumps(keys))
PY
SMALL_KEYS_FILE=$ST_KEYS_FILE
ST_KEYS_FILE="$ST_WORK/keys-big2.json"
rcp_key_step < /dev/null
ST_KEYS_FILE=$SMALL_KEYS_FILE
assert_eq "big key list reused the OpenWork key" "$API_TOKEN" "sk-aaaa1111bbbb2222cccc3333"

# 25. the standalone OpenCode install is gone, the flow is five steps, the
# config writer and the state repair stay
# The pattern escapes the separator so a plain grep for the removed names
# stays clean in this file too.
assert_eq "no standalone install source left" "$(grep -E -c 'OPENCODE_(INSTALL|RELEASE)|opencode\.ai/(v2/install|update)' src-cli/install.sh || true)" "0"
assert_eq "no standalone install helpers left" "$(grep -c 'install_opencode\|find_opencode\|oc_arch_name' src-cli/install.sh || true)" "0"
# 26. curl | bash leaves BASH_SOURCE unset and puts the script itself on stdin,
# so the entry guard must tolerate set -u and no prompt may read stdin directly.
assert_eq "entry guard is set -u safe" "$(grep -c 'BASH_SOURCE\[0\]:-}' src-cli/install.sh)" "1"
assert_eq "no prompt reads stdin directly" "$(grep -cE 'read -r( -t "[^"]+")? (reply|line)' src-cli/install.sh || true)" "0"
assert_eq "five step labels" "$(grep -o '\[[0-9]/[0-9]\]' src-cli/install.sh | sort -u | paste -sd' ')" "[1/5] [2/5] [3/5] [4/5] [5/5]"
assert_eq "config writer stays" "$(type -t write_opencode_config)" "function"
assert_eq "config schema stays" "$OC_CONFIG_SCHEMA" "https://opencode.ai/config.json"
assert_eq "state repair stays" "$(type -t repair_opencode_state)" "function"
openwork_selftest_reset
# shellcheck disable=SC2034
OS="linux"
# shellcheck disable=SC2034
ARCH="x86_64"
OPENWORK_FRESH_INSTALL=1
out=$(install_openwork 2>&1)
assert_eq "OpenWork step is two of five" "$(printf '%s\n' "$out" | grep -m1 '==>')" "==> [2/5] Installing OpenWork"

# 26. engine identity: fresh install picks v2, a present install keeps v1
openwork_selftest_reset
assert_eq "fresh install picks v2" "$(openwork_engine_name)" "opencode2"
OPENWORK_FRESH_INSTALL=0
assert_eq "present install picks v1" "$(openwork_engine_name)" "opencode"
: > "$CONFDIR/engine-v2-preview.json"
assert_eq "marker picks v2" "$(openwork_engine_name)" "opencode2"
rm -f "$CONFDIR/engine-v2-preview.json"
assert_eq "env picks v2" "$(OPENWORK_ENGINE_V2_PREVIEW=1 openwork_engine_name)" "opencode2"
assert_eq "blank env keeps v1" "$(OPENWORK_ENGINE_V2_PREVIEW='  ' openwork_engine_name)" "opencode"
assert_eq "target triple for linux x64" "$(engine_triple)" "x86_64-unknown-linux-gnu"
assert_eq "target triple for macos arm64" "$(OS=macos ARCH=aarch64 engine_triple)" "aarch64-apple-darwin"

# 27. locator: the installed sidecars dir, plain name first, then suffixed
openwork_selftest_reset
rm -rf "$ST_HOME/.local/share/openwork"
SIDE="$ST_HOME/.local/share/openwork/resources/sidecars"
mkdir -p "$SIDE"
printf '#!/bin/sh\n' > "$SIDE/opencode"
printf '#!/bin/sh\n' > "$SIDE/opencode-x86_64-unknown-linux-gnu"
assert_eq "plain engine name wins" "$(locate_openwork_engine)" "$SIDE/opencode"
rm -f "$SIDE/opencode"
assert_eq "triple suffix is the fallback" "$(locate_openwork_engine)" "$SIDE/opencode-x86_64-unknown-linux-gnu"
assert_eq "fresh install wants the v2 name" "$(OPENWORK_FRESH_INSTALL=1 locate_openwork_engine || true)" ""

# 27b. no engine anywhere: the locator fails, it never guesses
openwork_selftest_reset
rm -rf "$ST_HOME/.local/share/openwork" "$ST_HOME/.local/share/applications"
assert_fail "engine absent on linux" locate_openwork_engine

# 27c. the AppImage is extracted FUSE-free and the temp dir is recorded
openwork_selftest_reset
APPDIR="$ST_HOME/.local/share/openwork"
rm -rf "$APPDIR"
mkdir -p "$APPDIR"
AP="$APPDIR/OpenWork.AppImage"
cat > "$AP" <<'EOF'
#!/bin/sh
[ "$1" = "--appimage-extract" ] || exit 1
mkdir -p squashfs-root/resources/sidecars
printf '#!/bin/sh\n' > squashfs-root/resources/sidecars/opencode2
EOF
chmod 755 "$AP"
out=$(OPENWORK_FRESH_INSTALL=1 locate_openwork_engine)
assert_match "sidecar taken out of the appimage" "$out" '/squashfs-root/resources/sidecars/opencode2$'
assert_eq "extraction dir recorded for cleanup" "$([[ -d ${ENGINE_EXTRACT_DIR:-/nonexistent} ]] && printf yes || printf no)" "yes"
assert_match "extracted engine is under the temp dir" "$out" "^$ENGINE_EXTRACT_DIR/"
rm -rf "$ENGINE_EXTRACT_DIR"
# an appimage that extracts nothing is a locator miss, not a crash
printf '#!/bin/sh\nexit 1\n' > "$AP"
chmod 755 "$AP"
assert_fail "empty appimage extraction" locate_openwork_engine

# 27d. macOS reads the app bundle sidecars, with an overridable app path
openwork_selftest_reset
MACAPP="$ST_WORK/Applications/OpenWork.app"
mkdir -p "$MACAPP/Contents/Resources/sidecars"
printf '#!/bin/sh\n' > "$MACAPP/Contents/Resources/sidecars/opencode2"
assert_eq "macOS bundle sidecar found" "$(OS=macos ARCH=aarch64 OPENWORK_APP_DIR="$MACAPP" OPENWORK_FRESH_INSTALL=1 locate_openwork_engine)" "$MACAPP/Contents/Resources/sidecars/opencode2"

# 28. smoke: a missing engine skips with a warning and exits clean
openwork_selftest_reset
rm -rf "$ST_HOME/.local/share/openwork"
OPENWORK_FRESH_INSTALL=0
if err=$(smoke_test 2>&1 > /dev/null); then SMOKE_RC=0; else SMOKE_RC=$?; fi
assert_eq "missing engine is not a failure" "$SMOKE_RC" "0"
assert_match "skip warning" "$err" 'smoke test skipped: OpenWork-managed engine not found'
assert_match "skip says why" "$err" 'not found'

# 28b. smoke: the located engine runs with the standalone args and the key
ENGINE="$ST_HOME/.local/share/openwork/resources/sidecars/opencode"
mkdir -p "$(dirname "$ENGINE")"
cat > "$ENGINE" <<EOF
#!/bin/sh
printf '%s\n' "\$*" > "$ST_WORK/engine-args"
printf '%s\n' "\${EPFL_AIAAS_API_KEY:-none}" > "$ST_WORK/engine-env"
printf 'OK\n'
EOF
chmod 755 "$ENGINE"
API_TOKEN="sk-smoke"
out=$(smoke_test 2>&1)
assert_eq "engine invoked with the standalone args" "$(cat "$ST_WORK/engine-args")" 'run --standalone --model epfl-rcp-aiaas/epfl-aiaas/small Reply with the single word OK.'
assert_eq "engine gets the API key in the environment" "$(cat "$ST_WORK/engine-env")" "sk-smoke"
assert_match "smoke reports the reached answer" "$out" 'answered: OK'

# 28c. smoke: a fresh install runs the v2 sidecar, not the v1 one
SIDEDIR="$ST_HOME/.local/share/openwork/resources/sidecars"
cp "$ENGINE" "$SIDEDIR/opencode2"
chmod 755 "$SIDEDIR/opencode2"
printf '#!/bin/sh\necho v1-ran > "%s"\n' "$ST_WORK/v1-ran" > "$SIDEDIR/opencode"
chmod 755 "$SIDEDIR/opencode"
rm -f "$ST_WORK/v1-ran" "$ST_WORK/engine-args"
OPENWORK_FRESH_INSTALL=1
SMOKE_V2=$(smoke_test 2>&1)
assert_eq "v2 sidecar ran on a fresh install" "$(cat "$ST_WORK/engine-args" 2> /dev/null || true)" 'run --standalone --model epfl-rcp-aiaas/epfl-aiaas/small Reply with the single word OK.'
assert_eq "v1 sidecar never ran" "$([[ -f $ST_WORK/v1-ran ]] && printf yes || printf no)" "no"
assert_match "v2 answer reported" "$SMOKE_V2" 'answered: OK'

# 28d. smoke: a failing engine stays a hard failure
OPENWORK_FRESH_INSTALL=0
printf '#!/bin/sh\nexit 3\n' > "$ENGINE"
chmod 755 "$ENGINE"
assert_fail "failing engine is a failure" smoke_test

# 29. prompts end their line: the masking filter is line oriented, so a
# newline-less prompt stays in the sed buffer and the terminal shows nothing
# until the next line lands.
assert_eq "no inline read prompt left" "$(grep -c 'read -r -p' src-cli/install.sh || true)" "0"

# 30. the create-key flow announces the POST and the read-back around them
openwork_selftest_reset
SESSION_HEADER="sessionid=STUBSESS"
SESSION_CSRF="cookieCSRF"
out=$(create_key_flow "$(cat "$ST_KEYS_FILE")" <<< $'1\nOpenWork\n')
assert_match "create flow announces the POST" "$out" 'creating key OpenWork on the portal'
assert_eq "POST line comes before the result line" \
    "$(printf '%s\n' "$out" | awk '/creating key OpenWork on the portal/ { print "announce" } /key OpenWork created on the portal/ { print "created" }' | paste -sd'|')" \
    'announce|created'

printf 'selftest: %s checks passed\n' "$ASSERTS"
