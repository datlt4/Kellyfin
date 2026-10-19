#!/usr/bin/env bash
# Builds the mobile apps locally (same steps as .github/workflows/mobile.yml) into dist-mobile/.
#   scripts/build-mobile.sh android   signed APK (keystore: ~/.config/kellyfin/android-release.{jks,pass})
#   scripts/build-mobile.sh ios       unsigned IPA, macOS + Xcode only; re-signed on the Mac afterwards
set -euo pipefail
cd "$(dirname "$0")/.."
V=$(node -p "require('./package.json').version")
OUT=dist-mobile
mkdir -p "$OUT"
npm ci --no-audit --no-fund

android() {
  : "${ANDROID_HOME:=$HOME/Library/Android/sdk}"
  [ -d "$ANDROID_HOME" ] || ANDROID_HOME=$HOME/Android/Sdk
  NDK_HOME=${NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}
  if [ -z "${JAVA_HOME:-}" ]; then
    for j in "/Applications/Android Studio.app/Contents/jbr/Contents/Home" "$HOME/.local/jdk17"; do [ -d "$j" ] && JAVA_HOME=$j && break; done
  fi
  export ANDROID_HOME NDK_HOME JAVA_HOME PATH="$JAVA_HOME/bin:$PATH"
  rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android >/dev/null
  rm -rf src-tauri/gen/android
  npx tauri android init --ci
  cp -r src-tauri/icons/android/. src-tauri/gen/android/app/src/main/res/
  sh src-tauri/android/apply.sh
  npx tauri android build --apk --target aarch64 --target armv7 --target x86_64

  KS=$HOME/.config/kellyfin/android-release.jks
  [ -f "$KS" ] || { echo "missing $KS (the release keystore)"; exit 1; }
  APK=$(find src-tauri/gen/android -name '*universal-release-unsigned.apk' | head -1)
  BT=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)
  "$BT/zipalign" -f 4 "$APK" "$OUT/aligned.apk"
  "$BT/apksigner" sign --ks "$KS" --ks-pass "file:$HOME/.config/kellyfin/android-release.pass" \
    --out "$OUT/kellyfin_android_$V.apk" "$OUT/aligned.apk"
  rm "$OUT/aligned.apk"
  "$BT/apksigner" verify "$OUT/kellyfin_android_$V.apk" && echo "built $OUT/kellyfin_android_$V.apk"
}

ios() {
  npm run build
  rm -rf src-tauri/gen/apple
  npx tauri ios init --ci
  cp src-tauri/icons/ios/*.png src-tauri/gen/apple/Assets.xcassets/AppIcon.appiconset/
  for P in src-tauri/gen/apple/*_iOS/Info.plist; do
    /usr/libexec/PlistBuddy -c "Add :NSLocalNetworkUsageDescription string 'Kellyfin streams from your media server on the local network.'" "$P" || true
    /usr/libexec/PlistBuddy -c "Add :NSAppTransportSecurity dict" "$P" || true
    /usr/libexec/PlistBuddy -c "Add :NSAppTransportSecurity:NSAllowsArbitraryLoads bool true" "$P" || true
    /usr/libexec/PlistBuddy -c "Add :NSAppTransportSecurity:NSAllowsLocalNetworking bool true" "$P" || true
  done
  # Xcode's "Build Rust Code" phase needs `tauri ios build` (and signing): build the static lib here instead.
  IPHONEOS_DEPLOYMENT_TARGET=14.0 cargo build --manifest-path src-tauri/Cargo.toml --lib --release \
    --target aarch64-apple-ios --features tauri/custom-protocol
  mkdir -p src-tauri/gen/apple/Externals/arm64/release
  cp src-tauri/target/aarch64-apple-ios/release/libkellyfin_lib.a src-tauri/gen/apple/Externals/arm64/release/libapp.a
  perl -pi -e 's/^(\s*shellScript = ).*xcode-script.*$/$1"true";/' src-tauri/gen/apple/*.xcodeproj/project.pbxproj
  PROJ=$(ls -d src-tauri/gen/apple/*.xcodeproj | head -1)
  SCHEME=$(xcodebuild -list -json -project "$PROJ" | node -e 'const j=JSON.parse(require("fs").readFileSync(0));console.log(j.project.schemes.find(s=>/iOS/i.test(s))||j.project.schemes[0])')
  rm -rf build/Kellyfin.xcarchive
  xcodebuild -project "$PROJ" -scheme "$SCHEME" -configuration release -destination 'generic/platform=iOS' \
    -archivePath build/Kellyfin.xcarchive archive \
    CODE_SIGN_IDENTITY="" CODE_SIGNING_REQUIRED=NO CODE_SIGNING_ALLOWED=NO DEVELOPMENT_TEAM="" > build/xcodebuild.log 2>&1 ||
    { grep -E "error|ARCHIVE FAILED" build/xcodebuild.log | sort -u | head -20; exit 1; }
  APP=$(find build/Kellyfin.xcarchive/Products/Applications -maxdepth 1 -name '*.app' | head -1)
  rm -rf build/Payload && mkdir -p build/Payload && cp -R "$APP" build/Payload/
  (cd build && rm -f ../"$OUT/kellyfin_ios_${V}_unsigned.ipa" && zip -qr ../"$OUT/kellyfin_ios_${V}_unsigned.ipa" Payload)
  echo "built $OUT/kellyfin_ios_${V}_unsigned.ipa"
}

case "${1:-}" in
  android) android ;;
  ios) ios ;;
  *) echo "usage: $0 android|ios"; exit 2 ;;
esac
