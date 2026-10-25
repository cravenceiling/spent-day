# Spent Day for Android

A Jetpack Compose app on top of the Rust core. It runs on Android 8 or later.

## Build

You need:

- JDK 21.
- The Android SDK with platform 36 and NDK 28.2.13676358, the version `build-core.sh` uses. Gradle needs the SDK's location, from `ANDROID_HOME` or from `sdk.dir` in `local.properties`. `build-core.sh` looks for the NDK under `ANDROID_HOME`. If you use `local.properties` instead, or keep the NDK elsewhere, set `ANDROID_NDK_HOME` to the NDK too.
- Rust stable with `cargo-ndk` and the two Android targets.

```sh
sdkmanager "platform-tools" "platforms;android-36" "build-tools;36.0.0" "ndk;28.2.13676358"
cargo install cargo-ndk
rustup target add aarch64-linux-android x86_64-linux-android
```

Release builds need a signing key. Create one once and keep it outside the repo:

```sh
keytool -genkeypair -keystore ~/.android/spentday-release.jks -alias spentday \
  -keyalg RSA -keysize 4096 -validity 10000 -dname "CN=Spent Day"
```

It asks for a password.

Then add these lines to `~/.gradle/gradle.properties` in your home folder, with your own password. The keystore path must be absolute, because Gradle doesn't expand `~`. It is `/Users/you/.android/spentday-release.jks` on macOS and `/home/you/.android/spentday-release.jks` on Linux:

```properties
SPENTDAY_KEYSTORE=/Users/you/.android/spentday-release.jks
SPENTDAY_KEYSTORE_PASSWORD=your-keystore-password
```

Run `chmod 600 ~/.gradle/gradle.properties` so only you can read it. Don't put these lines in `apps/android/gradle.properties`, because that file is committed. You can set the same names as environment variables instead. `SPENTDAY_KEY_ALIAS` and `SPENTDAY_KEY_PASSWORD` are optional. They default to `spentday` and the keystore password. Then build and install:

```sh
cd apps/android
./gradlew assembleRelease
adb install -r app/build/outputs/apk/release/app-release.apk
```

Keep the same key for every update. Android refuses an update signed with a different key. For the same reason a release build won't install over a debug build. Uninstall the debug build first, which deletes its data.

For development, `./gradlew assembleDebug` builds `app-debug.apk`. Don't use it for real data. A debug build lets anyone with adb read its database, sync code included.

## Permissions

The app asks for Usage access, which it needs for per-app time. The website service is optional and off by default. If its switch is greyed out on Android 13 or later, open App info, then ⋮, then Allow restricted settings. Sites are recorded for Chrome, Firefox and DuckDuckGo. Other browsers get app time only, because Spent Day can't yet tell their private tabs apart.

The database is excluded from backups and device transfers.
