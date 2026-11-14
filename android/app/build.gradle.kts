plugins { id("com.android.application") }
android {
    namespace = "com.vinaywinai.databeam"
    compileSdk = 36
    defaultConfig {
        applicationId = "com.vinaywinai.databeam"
        minSdk = 26
        targetSdk = 36
        testInstrumentationRunner = "com.vinaywinai.databeam.SmokeInstrumentation"
        versionCode = 800
        versionName = "0.8.0"
        ndk { abiFilters += "arm64-v8a" }
    }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    packaging { jniLibs { useLegacyPackaging = true } }
}
// Fail before packaging an APK whose transfer engines would be missing.
val verifyEngines = tasks.register("verifyEngines") {
    doLast {
        for (name in listOf("libdatabeam_android.so", "libcroc.so")) {
            check(file("src/main/jniLibs/arm64-v8a/$name").isFile) { "Missing $name. Run python scripts/build.py --sdk <SDK path> first." }
        }
    }
}
tasks.matching { it.name == "preBuild" }.configureEach { dependsOn(verifyEngines) }

dependencies {
    implementation("org.rustls:rustls-platform-verifier:0.2.0")
    implementation("com.google.zxing:core:3.4.1")
    implementation("com.google.mlkit:barcode-scanning:17.3.0")
    implementation("androidx.camera:camera-camera2:1.6.2")
    implementation("androidx.camera:camera-lifecycle:1.6.2")
    implementation("androidx.camera:camera-view:1.6.2")
    implementation("androidx.activity:activity:1.10.1")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20250517")
}
