plugins {
  id("com.android.library")
  id("org.jetbrains.kotlin.android")
}

android {
  namespace = "dev.hoangmirs.sharefile"
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
  }
}

dependencies {
  implementation(project(":tauri-android"))
  // The versions tauri-android declares, so the plugin adds no newer AndroidX
  // to an app.
  implementation("androidx.core:core:1.7.0")
  implementation("androidx.appcompat:appcompat:1.6.0")
}
