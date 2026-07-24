plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "tokyo.runo.openwebserver"
    compileSdk = 35

    defaultConfig {
        applicationId = "tokyo.runo.openwebserver"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        // 実機のスマホ/タブレットは`arm64-v8a`(2026年時点の主流ABI)、
        // x86_64はこの開発機のAVD(Pixel_9_Pro、Google Play系エミュレータ
        // イメージはx86_64)で実機能検証するために追加した(2026-07-24、
        // 実エミュレータでの`/healthz`応答確認のために必須と判明——
        // arm64-v8a単体のjniLibsではx86_64エミュレータの
        // `nativeLibraryDir`にネイティブバイナリが展開されず、
        // 「native binary not found」で起動に失敗した実測結果を受けての
        // 追加)。
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    // 正式リリース署名(2026-07-24新設)。実際の値(keystoreパス・パスワード・
    // エイリアス)は環境変数経由でのみ受け取り、このファイル自体には平文の
    // 秘密情報を一切書かない。環境変数が未設定の場合、releaseビルドタイプは
    // 署名設定なしのまま(既存のデバッグ署名フローには影響しない)。
    // 使い方: 環境変数 OPEN_WEB_SERVER_RELEASE_STORE_FILE /
    // OPEN_WEB_SERVER_RELEASE_STORE_PASSWORD /
    // OPEN_WEB_SERVER_RELEASE_KEY_ALIAS /
    // OPEN_WEB_SERVER_RELEASE_KEY_PASSWORD を設定した上で
    // `gradle :app:assembleRelease` を実行する。
    // keystoreファイル自体は絶対にこのリポジトリへコミットしないこと
    // (.gitignoreに追加済み)。
    val releaseStoreFile = System.getenv("OPEN_WEB_SERVER_RELEASE_STORE_FILE")
    val releaseStorePassword = System.getenv("OPEN_WEB_SERVER_RELEASE_STORE_PASSWORD")
    val releaseKeyAlias = System.getenv("OPEN_WEB_SERVER_RELEASE_KEY_ALIAS")
    val releaseKeyPassword = System.getenv("OPEN_WEB_SERVER_RELEASE_KEY_PASSWORD")
    val hasReleaseSigningEnv = !releaseStoreFile.isNullOrBlank() &&
        !releaseStorePassword.isNullOrBlank() &&
        !releaseKeyAlias.isNullOrBlank() &&
        !releaseKeyPassword.isNullOrBlank()

    signingConfigs {
        if (hasReleaseSigningEnv) {
            create("release") {
                storeFile = file(releaseStoreFile!!)
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            if (hasReleaseSigningEnv) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        viewBinding = false
    }

    // 既定(AGP/Android 6.0+)ではネイティブライブラリはAPK内から直接実行
    // され、`nativeLibraryDir`配下には展開されない(`status=run-from-apk`)。
    // 本アプリは`ProcessBuilder`で実ファイルパスとして起動する必要がある
    // ため、旧来通りインストール時に展開される動作を明示的に強制する
    // (2026-07-24、実機検証で`nativeLibraryDir`が空だったため発覚・追加)。
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    // DuckDNSトークンの安全な永続化(2026-07-24追加)。平文SharedPreferences
    // には保存せず、Android推奨のEncryptedSharedPreferences経由で保存する。
    implementation("androidx.security:security-crypto:1.1.0-alpha06")
}
