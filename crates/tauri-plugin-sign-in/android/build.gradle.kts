plugins {
  id("com.android.library")
  id("org.jetbrains.kotlin.android")
}

android {
  namespace = "dev.hoangmirs.signin"
  compileSdk = 36

  defaultConfig {
    minSdk = 26
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
  // Tauri's Android template compiles with Kotlin 1.9, which reads Kotlin
  // metadata up to 2.0. Credentials 1.6 and googleid 1.2 carry 2.1 and later,
  // and an app on the template fails to build with them.
  implementation("androidx.credentials:credentials:1.5.0")
  implementation("androidx.credentials:credentials-play-services-auth:1.5.0")
  implementation("com.google.android.libraries.identity.googleid:googleid:1.1.1")
  // Spike: Apple on Android through an Auth Tab.
  implementation("androidx.browser:browser:1.9.0")
}
