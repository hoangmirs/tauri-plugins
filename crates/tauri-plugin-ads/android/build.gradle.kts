plugins {
  id("com.android.library")
  id("org.jetbrains.kotlin.android")
}

android {
  namespace = "dev.hoangmirs.ads"
  compileSdk = 36

  defaultConfig {
    minSdk = 24
  }

  compileOptions {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
  }
  kotlinOptions {
    jvmTarget = "17"
    // Google's ads jars carry newer Kotlin metadata than a Tauri app's Kotlin
    // 1.9 reads. The plugin calls only their Java surface, as the Firebase
    // plugin does, so skipping the check is safe here.
    freeCompilerArgs += listOf("-Xskip-metadata-version-check")
  }
}

dependencies {
  implementation(project(":tauri-android"))
  implementation("com.google.android.gms:play-services-ads:25.5.0")
  implementation("com.google.android.ump:user-messaging-platform:4.0.0")
}
