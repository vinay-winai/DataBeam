# Keep Rust JNI entrypoints and platform TLS verifier callbacks intact under R8.
-keep class com.vinaywinai.databeam.NativeBridge { *; }
-keep class org.rustls.platformverifier.** { *; }
