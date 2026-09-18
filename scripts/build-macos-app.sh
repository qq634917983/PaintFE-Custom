#!/bin/sh
set -eu

PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
APP_NAME="PaintFE"
VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$PROJECT_DIR/Cargo.toml" | head -n 1)
APP_DIR="$PROJECT_DIR/dist/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
ICONSET_DIR="$PROJECT_DIR/target/macos/AppIcon.iconset"
SOURCE_ICON="$PROJECT_DIR/assets/icons/app_icon.png"

if [ "${1:-}" != "--skip-build" ]; then
    cargo build --release
fi

if [ ! -x "$PROJECT_DIR/target/release/$APP_NAME" ]; then
    echo "Missing release binary: target/release/$APP_NAME" >&2
    exit 1
fi

mkdir -p "$MACOS_DIR" "$RESOURCES_DIR" "$ICONSET_DIR"
cp "$PROJECT_DIR/target/release/$APP_NAME" "$MACOS_DIR/$APP_NAME"
chmod 755 "$MACOS_DIR/$APP_NAME"

for size in 16 32 128 256 512; do
    double_size=$((size * 2))
    sips -z "$size" "$size" "$SOURCE_ICON" --out "$ICONSET_DIR/icon_${size}x${size}.png" >/dev/null
    sips -z "$double_size" "$double_size" "$SOURCE_ICON" --out "$ICONSET_DIR/icon_${size}x${size}@2x.png" >/dev/null
done
ICON_FILE="AppIcon.icns"
if ! iconutil -c icns "$ICONSET_DIR" -o "$RESOURCES_DIR/$ICON_FILE"; then
    # Some macOS versions reject iconsets enlarged from a source below 1024 px.
    # Finder also accepts a PNG named by CFBundleIconFile.
    ICON_FILE="AppIcon.png"
    cp "$SOURCE_ICON" "$RESOURCES_DIR/$ICON_FILE"
fi

cat > "$CONTENTS_DIR/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIconFile</key>
    <string>$ICON_FILE</string>
    <key>CFBundleIdentifier</key>
    <string>com.paintfe.PaintFE</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
</dict>
</plist>
EOF

plutil -lint "$CONTENTS_DIR/Info.plist"
codesign --force --deep --sign - "$APP_DIR"
echo "Created $APP_DIR"
