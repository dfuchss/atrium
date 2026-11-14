#!/usr/bin/env bash
# Takes the README screenshots (docs/screenshots/*.png) with demo data only.
#
# Runs a throwaway Matrix homeserver (tuwunel) and Element web in Docker, seeds demo users and
# chats, then starts a separate Atrium instance (debug bundle) with its own config folder and
# captures only its window. The regular Atrium installation and its accounts are not touched:
# the demo bundle has its own bundle id (org.fuchss.atrium.demo), so WebKit data, logs, caches
# and its WebCrypto Keychain item are separate (no Keychain prompt), and it uses the development
# hooks described in README.md ("Development"). Everything of the demo id is deleted at the end.
#
# Needs macOS, docker, cargo tauri, trunk, python3, magick (ImageMagick) and Screen Recording
# permission for the terminal. Ports 8008 and 8080 must be free.
#
# Environment:
#   ATRIUM_SHOTS_TARGET_DIR  cargo target folder for the demo bundle (default target/screenshots,
#                            so a running debug bundle in target/debug is not replaced)
#   ATRIUM_SHOTS_SKIP_BUILD  1 = reuse the bundle built by an earlier run
#   ATRIUM_SHOTS_KEEP_SECONDS  N = no screenshots: keep the demo running for N seconds, then print
#                            its unread log lines (checks the unread rules against a real Element)
#   ATRIUM_SHOTS_WINDOW_POSITION  window position "x,y" (default 24,48 on the main display)
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$REPO/docs/screenshots"
TARGET_DIR="${ATRIUM_SHOTS_TARGET_DIR:-$REPO/target/screenshots}"
APP_BIN="$TARGET_DIR/debug/bundle/macos/Atrium.app/Contents/MacOS/atrium"
TUWUNEL_IMAGE="ghcr.io/matrix-construct/tuwunel:v1.9.3"
ELEMENT_IMAGE="ghcr.io/element-hq/element-web:v1.12.35"
SERVER_NAME="localhost:8008"
HS_URL="http://localhost:8008"
ELEMENT_HOST="localhost:8080"
CONTAINER_PREFIX="atrium-demo"
# Bundle id of the demo build; never the regular org.fuchss.atrium.
DEMO_ID="org.fuchss.atrium.demo"
# Logical window content size is 1280x800 (see main.rs); images are scaled down to this width.
MAX_WIDTH=1600
# Window position on the main display (Retina on a MacBook, so captures are 2x).
WINDOW_POSITION="${ATRIUM_SHOTS_WINDOW_POSITION:-24,48}"

WORK="$(mktemp -d /tmp/atrium-demo.XXXXXX)"
CONFIG="$WORK/config"
DEMO_PID=""

log() { printf '\033[1m==> %s\033[0m\n' "$*"; }

stop_demo() {
    if [[ -n "$DEMO_PID" ]] && kill -0 "$DEMO_PID" 2>/dev/null; then
        kill "$DEMO_PID" 2>/dev/null || true
        for _ in $(seq 1 20); do
            kill -0 "$DEMO_PID" 2>/dev/null || break
            sleep 0.5
        done
        kill -9 "$DEMO_PID" 2>/dev/null || true
        wait "$DEMO_PID" 2>/dev/null || true
    fi
    DEMO_PID=""
}

# Deletes everything stored under the demo bundle id (data, WebKit stores, logs, caches,
# preferences, WebCrypto master key), so every run starts fresh and prompt-free.
remove_demo_data() {
    [[ "$DEMO_ID" == org.fuchss.atrium.demo ]] || return 1
    local lib="$HOME/Library"
    rm -rf "$lib/Application Support/$DEMO_ID" "$lib/WebKit/$DEMO_ID" "$lib/Logs/$DEMO_ID" \
        "$lib/Caches/$DEMO_ID" "$lib/HTTPStorages/$DEMO_ID" "$lib/HTTPStorages/$DEMO_ID.binarycookies" \
        "$lib/Saved Application State/$DEMO_ID.savedState"
    rm -f "$lib/Preferences/$DEMO_ID.plist"
    security delete-generic-password -a "com.apple.WebKit.WebCrypto.master+$DEMO_ID" >/dev/null 2>&1 || true
}

cleanup() {
    set +e
    log "Tearing down"
    stop_demo
    docker rm -f "$CONTAINER_PREFIX-tuwunel" "$CONTAINER_PREFIX-element" >/dev/null 2>&1
    remove_demo_data
    rm -rf "$WORK"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------------------------
log "Building the demo bundle $DEMO_ID ($TARGET_DIR)"
PLIST="$TARGET_DIR/debug/bundle/macos/Atrium.app/Contents/Info.plist"
if [[ "${ATRIUM_SHOTS_SKIP_BUILD:-0}" != 1 || ! -x "$APP_BIN" ]] ||
    [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$PLIST" 2>/dev/null)" != "$DEMO_ID" ]]; then
    (cd "$REPO/app/src-tauri" &&
        CARGO_TARGET_DIR="$TARGET_DIR" cargo tauri build --debug --config "{\"identifier\":\"$DEMO_ID\"}")
fi
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$PLIST")" == "$DEMO_ID" ]] ||
    { echo "demo bundle does not have the id $DEMO_ID" >&2; exit 1; }
# Leftovers of an aborted run.
remove_demo_data

# ---------------------------------------------------------------------------------------------
log "Starting tuwunel and Element web"
docker rm -f "$CONTAINER_PREFIX-tuwunel" "$CONTAINER_PREFIX-element" >/dev/null 2>&1 || true
docker run -d --rm --name "$CONTAINER_PREFIX-tuwunel" -p 127.0.0.1:8008:8008 \
    -e TUWUNEL_SERVER_NAME="$SERVER_NAME" \
    -e TUWUNEL_ADDRESS='["0.0.0.0"]' \
    -e TUWUNEL_PORT=8008 \
    -e TUWUNEL_DATABASE_PATH=/var/lib/tuwunel \
    -e TUWUNEL_ALLOW_REGISTRATION=true \
    -e TUWUNEL_YES_I_AM_VERY_VERY_SURE_I_WANT_AN_OPEN_REGISTRATION_SERVER_PRONE_TO_ABUSE=true \
    -e TUWUNEL_ALLOW_FEDERATION=false \
    -e TUWUNEL_SHOW_ALL_LOCAL_USERS_IN_USER_DIRECTORY=true \
    -e TUWUNEL_NEW_USER_DISPLAYNAME_SUFFIX= \
    -e TUWUNEL_LOG=warn \
    "$TUWUNEL_IMAGE" >/dev/null

mkdir -p "$WORK/element"
cat >"$WORK/element/config.json" <<JSON
{
  "default_server_config": {
    "m.homeserver": { "base_url": "$HS_URL", "server_name": "$SERVER_NAME" }
  },
  "brand": "Element",
  "disable_custom_urls": true,
  "disable_guests": true,
  "disable_3pid_login": true,
  "disable_login_language_selector": true,
  "show_labs_settings": false,
  "room_directory": { "servers": ["$SERVER_NAME"] },
  "setting_defaults": {
    "UIFeature.feedback": false,
    "UIFeature.registration": false,
    "UIFeature.passwordReset": false,
    "UIFeature.identityServer": false,
    "UIFeature.thirdPartyId": false,
    "UIFeature.shareSocial": false,
    "UIFeature.deactivate": false,
    "deviceClientInformationOptIn": false,
    "promptBeforeInviteUnknownUsers": false,
    "releaseAnnouncementData": { "room_list_section": true }
  }
}
JSON
docker run -d --rm --name "$CONTAINER_PREFIX-element" -p 127.0.0.1:8080:8080 \
    -e ELEMENT_WEB_PORT=8080 \
    -v "$WORK/element/config.json:/app/config.json:ro" \
    "$ELEMENT_IMAGE" >/dev/null

for url in "$HS_URL/_matrix/client/versions" "http://$ELEMENT_HOST/config.json"; do
    for i in $(seq 1 60); do
        curl -sf "$url" >/dev/null && break
        [[ $i == 60 ]] && { echo "timeout waiting for $url" >&2; exit 1; }
        sleep 1
    done
done

# ---------------------------------------------------------------------------------------------
log "Seeding demo users, rooms and messages"
python3 -I - "$HS_URL" "$SERVER_NAME" "$ELEMENT_HOST" "$WORK/element-session.js" <<'PY'
import json, sys, time, urllib.error, urllib.parse, urllib.request

hs, server, element_host, script_path = sys.argv[1:5]
PASSWORD = "atrium-demo-password"


def call(method, path, body=None, token=None, ok=(200,)):
    req = urllib.request.Request(hs + "/_matrix/client/v3" + path, method=method)
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    data = json.dumps(body if body is not None else {}).encode()
    try:
        with urllib.request.urlopen(req, data if method != "GET" else None) as r:
            return r.status, json.load(r)
    except urllib.error.HTTPError as e:
        payload = json.load(e)
        if e.code in ok:
            return e.code, payload
        raise SystemExit(f"{method} {path}: {e.code} {payload}")


def register(user, display):
    body = {"username": user, "password": PASSWORD, "inhibit_login": False}
    status, res = call("POST", "/register", body, ok=(200, 401))
    if status == 401:
        body["auth"] = {"type": "m.login.dummy", "session": res.get("session")}
        _, res = call("POST", "/register", body)
    token = res["access_token"]
    call("PUT", f"/profile/{q(res['user_id'])}/displayname", {"displayname": display}, token)
    return {"id": res["user_id"], "token": token}


def q(s):
    return urllib.parse.quote(s, safe="")


txn = 0


def send(room, user, body, formatted=None):
    global txn
    txn += 1
    content = {"msgtype": "m.text", "body": body}
    if formatted:
        content.update(format="org.matrix.custom.html", formatted_body=formatted)
    _, res = call("PUT", f"/rooms/{q(room)}/send/m.room.message/demo{txn}", content, user["token"])
    time.sleep(0.3)  # distinct timestamps keep the timeline order stable
    return res["event_id"]


def room(creator, name, members, topic=None, direct=False):
    body = {"preset": "trusted_private_chat" if direct else "private_chat",
            "invite": [m["id"] for m in members], "is_direct": direct}
    if name:
        body["name"] = name
    if topic:
        body["topic"] = topic
    _, res = call("POST", "/createRoom", body, creator["token"])
    rid = res["room_id"]
    for m in members:
        call("POST", f"/rooms/{q(rid)}/join", {}, m["token"])
    return rid


def read_up_to(user, rid, event):
    call("POST", f"/rooms/{q(rid)}/read_markers", {"m.fully_read": event, "m.read": event}, user["token"])


# The first user of a fresh server becomes its admin and gets the admin room: keep that away
# from the demo users.
register("server-admin", "Server Admin")
# Demo people go by first names only.
ada = register("ada", "Ada")
grace = register("grace", "Grace")
linus = register("linus", "Linus")
margaret = register("margaret", "Margaret")

design = room(margaret, "Design", [ada, grace], "Icons, colours and layout")
send(design, ada, "Could you share the new icon drafts when they are ready?")
read_up_to(ada, design, send(design, margaret, "Sure, finishing them this afternoon."))

coffee = room(linus, "Coffee ☕", [ada, grace, margaret], "Breaks, cake and other important things")
send(coffee, grace, "Who took the last croissant? \U0001F950")
read_up_to(ada, coffee, send(coffee, ada, "Guilty. I'll bring new ones tomorrow!"))

dm = room(grace, None, [ada], direct=True)
read_up_to(ada, dm, send(dm, ada, "Thanks for the review yesterday!"))
call("PUT", f"/user/{q(ada['id'])}/account_data/m.direct", {grace["id"]: [dm]}, ada["token"])
call("PUT", f"/user/{q(grace['id'])}/account_data/m.direct", {ada["id"]: [dm]}, grace["token"])

project = room(ada, "Project Atrium", [grace, linus, margaret], "One window for all your messengers")
conversation = [
    (grace, "Morning everyone \U0001F44B The new sidebar build is up.", None),
    (linus, "Nice! Each account really gets its own data store now?", None),
    (ada, "Yes, every account is isolated, so two Slack workspaces work side by side.", None),
    (margaret, "I tested notifications: badges, the dock count and *Do Not Disturb* all behave ✅",
     "I tested notifications: badges, the dock count and <em>Do Not Disturb</em> all behave ✅"),
    (grace, "And sleeping accounts free their memory until you open them again \U0001F319", None),
    (ada, "Plan for the release:\n\n1. Finish the **settings dialog**\n2. Polish the README\n3. Screenshots with demo data only \U0001F4F8",
     "<p>Plan for the release:</p>\n<ol>\n<li>Finish the <strong>settings dialog</strong></li>\n<li>Polish the README</li>\n<li>Screenshots with demo data only \U0001F4F8</li>\n</ol>"),
    (linus, "I'll take the screenshots. Throwaway homeserver, no real accounts.", None),
    (margaret, "Can custom services stay plain TOML files?", None),
    (ada, "Yes: drop a file into the `services` folder and press *Reload*.",
     "Yes: drop a file into the <code>services</code> folder and press <em>Reload</em>."),
    (grace, "Love it. \U0001F680", None),
    (linus, "Release notes are in the draft, have a look when you can.", None),
    (ada, "Thanks, everyone! \U0001F389", None),
]
last = None
for user, body, html in conversation:
    last = send(project, user, body, html)
read_up_to(ada, project, last)

# Unread for Ada in the rooms that are not open: badges in Element and in the Atrium sidebar.
send(design, margaret, "New icon sketches are in the shared folder \U0001F3A8")
send(design, grace, "The atrium from above looks great! Maybe a bit more contrast in dark mode?")
send(coffee, linus, "Coffee at 3? ☕")
send(dm, grace, "Do you have a minute later? Quick question about link handling.")

# Element's release announcements ("Introducing sections") count as seen.
call("PUT", f"/user/{q(ada['id'])}/account_data/im.vector.web.settings",
     {"releaseAnnouncementData": {"room_list_section": True}}, ada["token"])

# A separate Element session for Ada, seeded into Element's legacy plaintext storage keys.
_, login = call("POST", "/login", {
    "type": "m.login.password",
    "identifier": {"type": "m.id.user", "user": "ada"},
    "password": PASSWORD,
    "device_id": "ATRIUMDEMO",
    "initial_device_display_name": "Atrium demo",
})
seed = {
    "mx_hs_url": hs,
    "mx_user_id": login["user_id"],
    "mx_access_token": login["access_token"],
    "mx_device_id": login["device_id"],
    "mx_is_guest": "false",
    "mx_last_room_id": project,
    "notifications_hidden": "true",
}
with open(script_path, "w") as f:
    f.write(f"""// Demo Element session (screenshots); acts only on the local demo Element.
(() => {{
  if (location.origin !== {json.dumps("http://" + element_host)}) return;
  // Seed the session once (Element moves the token to IndexedDB on first load).
  try {{
    if (!localStorage.getItem("mx_user_id")) {{
      const seed = {json.dumps(seed)};
      for (const [k, v] of Object.entries(seed)) localStorage.setItem(k, v);
    }}
  }} catch (e) {{}}
  // First-run prompts of a fresh session (verify this device, recovery, notifications, release
  // announcements) are dismissed with their own "Later"/"Dismiss"/"OK" buttons, and toasts stay
  // hidden.
  const hide = () => {{
    const style = document.createElement("style");
    style.textContent = ".mx_ToastContainer {{ display: none !important; }}";
    document.head.appendChild(style);
  }};
  if (document.head) hide(); else document.addEventListener("DOMContentLoaded", hide);
  const dismiss = /^(later|i'll verify later|dismiss|not now|skip|no thanks|close|cancel|ok|got it)$/i;
  const sweep = () => {{
    for (const box of document.querySelectorAll(".mx_Toast_toast, .mx_Dialog, [role=dialog]")) {{
      for (const b of box.querySelectorAll("button, [role=button]")) {{
        const label = (b.getAttribute("aria-label") || b.textContent || "").trim();
        if (dismiss.test(label)) {{ b.click(); break; }}
      }}
    }}
  }};
  const started = Date.now();
  const timer = setInterval(() => {{
    sweep();
    if (Date.now() - started > 120000) clearInterval(timer);
  }}, 500);
}})();
""")
print(f"seeded: {login['user_id']}, rooms {project} {design} {coffee} {dm}")
PY

# ---------------------------------------------------------------------------------------------
log "Writing the demo config ($CONFIG)"
mkdir -p "$CONFIG/services"
python3 -I - "$REPO/services/element.toml" "$CONFIG" "$ELEMENT_HOST" <<'PY'
import base64, json, re, sys, urllib.request, uuid
element_toml, config, element_host = sys.argv[1:4]

# Local http Element: the built-in definition with an http URL and the local host.
toml = open(element_toml).read()
toml = re.sub(r'(?m)^id = .*$', 'id = "element-demo"', toml)
toml = re.sub(r'(?m)^url = .*$', 'url = "http://{instance}/"', toml)
toml = re.sub(r'(?m)^instance_default = .*$', f'instance_default = "{element_host}"', toml)
toml = re.sub(r'(?m)^internal_hosts = .*$', 'internal_hosts = ["localhost"]', toml)
open(f"{config}/services/element-demo.toml", "w").write(
    "# Demo service for the README screenshots: Element web on localhost.\n" + toml)


def account(service, label, **extra):
    return {"id": str(uuid.uuid4()), "service": service, "label": label, **extra}


def element_favicon():
    """The icon the app picks on a real Element web (the largest rel="icon" image), taken from the
    local Element: the app fetches favicons over https only, so it never loads the http demo's."""
    page = urllib.request.urlopen(f"http://{element_host}/").read().decode()
    icons = []
    for link in re.findall(r"<link\b[^>]*>", page):
        rel = re.search(r'rel="([^"]*)"', link)
        href = re.search(r'href="([^"]*)"', link)
        size = re.search(r'sizes="(\d+)x\d+"', link)
        if rel and href and size and "icon" in rel.group(1).split():
            icons.append((int(size.group(1)), href.group(1)))
    _, href = max(icons)
    with urllib.request.urlopen(f"http://{element_host}/{href}") as response:
        mime = response.headers.get_content_type()
        return f"data:{mime};base64," + base64.b64encode(response.read()).decode()


# Site favicons as in a regular installation, no Simple Icons glyphs.
accounts = [
    account("element-demo", "Ada", instance=element_host, favicon=element_favicon()),
    # No Slack: its sign-in page offers no site icon. WhatsApp goes to sleep one minute after
    # the start (moon and greyscale icon).
    account("discord", "Friends"),
    account("mastodon", "mastodon.social", instance="mastodon.social"),
    account("whatsapp", "Family", hibernate_after_min=1),
    # No Microsoft Teams: its sign-in page makes WebKit ask (system dialog) whether related
    # Microsoft sites may share cookies.
    account("bluesky", "Bluesky"),
]
persisted = {
    "accounts": accounts,
    "settings": {
        "active": accounts[0]["id"],
        # No notifications, login item or extra menu bar icon from the demo instance.
        "dnd": True,
        "launch_at_login": False,
        "show_tray": False,
        "icon_size": "medium",
    },
    "custom_services": [{
        "id": "custom-team-wiki",
        "name": "Team Wiki",
        "url": "https://{instance}/",
        "color": "#0F766E",
        "instance_label": "Host",
        "title_regex": None,
        "user_agent": None,
    }],
}
json.dump(persisted, open(f"{config}/accounts.json", "w"), indent=2)
PY

# ---------------------------------------------------------------------------------------------
start_demo() { # $1: UI query (ATRIUM_DEV_UI_QUERY)
    ATRIUM_CONFIG_DIR="$CONFIG" \
        ATRIUM_ALWAYS_ON_TOP=1 \
        ATRIUM_WINDOW_POSITION="$WINDOW_POSITION" \
        ATRIUM_DEV_INIT_SCRIPT="$WORK/element-session.js" \
        ATRIUM_DEV_UI_QUERY="$1" \
        "$APP_BIN" >>"$WORK/demo.out" 2>&1 &
    DEMO_PID=$!
}

# CGWindowID of the largest on-screen window of the demo process (never the user's instance).
window_id() {
    osascript -l JavaScript - "$DEMO_PID" <<'JS'
ObjC.import("CoreGraphics");
function run(argv) {
  const pid = Number(argv[0]);
  const list = ObjC.deepUnwrap(ObjC.castRefToObject(
    $.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly, $.kCGNullWindowID)));
  let best = null, area = 0;
  for (const w of list) {
    if (w.kCGWindowOwnerPID !== pid || w.kCGWindowOwnerName !== "Atrium") continue;
    const a = w.kCGWindowBounds.Width * w.kCGWindowBounds.Height;
    if (a > area) { area = a; best = w.kCGWindowNumber; }
  }
  return best === null ? "" : String(best);
}
JS
}

capture() { # $1: output name, $2: UI query, $3: seconds to wait after the start
    local name="$1" query="$2" wait="$3" wid=""
    log "Capturing $name.png (query '${query}', ${wait}s)"
    start_demo "$query"
    for _ in $(seq 1 60); do
        wid="$(window_id)"
        [[ -n "$wid" ]] && break
        sleep 1
    done
    [[ -n "$wid" ]] || { echo "demo window not found" >&2; exit 1; }
    sleep "$wait"
    wid="$(window_id)"
    screencapture -x -o -l"$wid" "$WORK/$name.png"
    echo "captured window $wid: $(magick identify -format "%wx%h" "$WORK/$name.png") px"
    magick "$WORK/$name.png" -resize "${MAX_WIDTH}x>" -strip "$OUT/$name.png"
    stop_demo
}

if [[ -n "${ATRIUM_SHOTS_KEEP_SECONDS:-}" ]]; then
    log "Running the demo for ${ATRIUM_SHOTS_KEEP_SECONDS}s"
    start_demo ""
    sleep "$ATRIUM_SHOTS_KEEP_SECONDS"
    log "Unread log lines"
    grep -h -E "unread|poll" "$CONFIG/atrium.log" 2>/dev/null | tail -40 || true
    exit 0
fi

mkdir -p "$OUT"
# The first start logs Ada in; the main shot waits for the sleeping accounts (1 min).
capture main "" 90
capture settings-accounts "dialog=accounts&edit=0" 15
capture settings-general "dialog=general" 10
capture custom-services "dialog=custom" 10

log "Done: $OUT"
