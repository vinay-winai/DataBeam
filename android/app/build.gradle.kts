import java.util.Properties
plugins { id("com.android.application") }
abstract class BundleGuideAsset : DefaultTask() {
    @get:InputFile abstract val sourceFile:org.gradle.api.file.RegularFileProperty
    @get:OutputDirectory abstract val outputDirectory:org.gradle.api.file.DirectoryProperty
    @TaskAction fun generate(){
        val directory=outputDirectory.get().asFile;directory.mkdirs()
        sourceFile.get().asFile.copyTo(directory.resolve("troubleshoot.md"),overwrite=true)
    }
}
val bundleGuide=tasks.register<BundleGuideAsset>("bundleGuide") {
    sourceFile.set(rootProject.file("../troubleshoot.md"));outputDirectory.set(layout.buildDirectory.dir("generated/guideAssets"))
}
val releaseSigningFile=rootProject.file(".tools/signing/release.properties")
val releaseSigning=Properties().apply{if(releaseSigningFile.exists())releaseSigningFile.inputStream().use{load(it)}}
android {
    namespace = "com.vinaywinai.databeam"
    compileSdk = 36
    sourceSets.getByName("main").assets.srcDir(rootProject.file("../assets/wordlists"))
    sourceSets.getByName("test").resources.srcDir(rootProject.file("../assets/wordlists"))
    defaultConfig {
        applicationId = "com.vinaywinai.databeam"
        minSdk = 26
        targetSdk = 36
        testInstrumentationRunner = "com.vinaywinai.databeam.SmokeInstrumentation"
        versionCode = 808
        versionName = "0.8.8"
        ndk { abiFilters += "arm64-v8a" }
    }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    if(releaseSigningFile.exists())signingConfigs.create("publicRelease"){
        storeFile=rootProject.file(".tools/signing/databeam-release.jks")
        storePassword=releaseSigning.getProperty("storePassword");keyAlias=releaseSigning.getProperty("keyAlias");keyPassword=releaseSigning.getProperty("keyPassword")
    }
    buildTypes.getByName("release"){
        if(releaseSigningFile.exists())signingConfig=signingConfigs.getByName("publicRelease")
    }
    packaging { jniLibs { useLegacyPackaging = true } }
}
androidComponents.onVariants {variant->variant.sources.assets?.addGeneratedSourceDirectory(bundleGuide,BundleGuideAsset::outputDirectory)}
// Fail before packaging an APK whose transfer engines would be missing.
val verifyEngines = tasks.register("verifyEngines") {
    doLast {
        for (name in listOf("libdatabeam_android.so", "libcroc.so")) {
            check(file("src/main/jniLibs/arm64-v8a/$name").isFile) { "Missing $name. Run python scripts/build.py --sdk <SDK path> first." }
        }
    }
}
tasks.matching { it.name == "preBuild" }.configureEach { dependsOn(verifyEngines,bundleGuide) }

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
