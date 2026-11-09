import org.gradle.api.tasks.Exec

plugins {
	id("com.android.application")
}

// Where Cargo builds. Keep it out of the source tree (it is large); the default
// is under the Gradle build directory. CARGO_TARGET_DIR overrides it.
val cargoTarget = providers.environmentVariable("CARGO_TARGET_DIR")
	.orElse(layout.buildDirectory.dir("cargo-target").map { it.asFile.absolutePath })
// Comma separated; add x86_64 for an Intel-host emulator.
val abis = providers.gradleProperty("relay.abis").orElse("arm64-v8a").get().split(',').map { it.trim() }
val rustDir = rootProject.projectDir
val jniOut = layout.buildDirectory.dir("rustJniLibs")
val bindingsOut = layout.buildDirectory.dir("generated/uniffi")
val minSdkLevel = 28
val relayVersion = providers.gradleProperty("relay.version").orElse("0.0.0-dev").get()
val relayVersionCode = Regex("""^(\d+)\.(\d+)\.(\d+)""").find(relayVersion)
	?.destructured?.let { (major, minor, patch) -> major.toInt() * 10000 + minor.toInt() * 100 + patch.toInt() }
	?: 1

android {
	namespace = "dev.relay.app"
	compileSdk = 35
	providers.environmentVariable("ANDROID_NDK_HOME").orNull?.let { ndkPath = it }

	defaultConfig {
		applicationId = "dev.relay.app"
		minSdk = minSdkLevel
		targetSdk = 35
		// Android refuses an install whose versionCode is not higher, so it follows the version:
		// 0.1.0 is 100, 1.2.3 is 10203. A pre-release suffix (0.1.0-rc.1) maps to its base version.
		versionCode = relayVersionCode
		versionName = relayVersion
		ndk { abiFilters += abis }
		testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
	}

	// The release key comes from the environment (CI's android-signing environment); it is never
	// in the repository. Without it there is no release signing and assembleRelease is refused.
	val keystore = providers.environmentVariable("RELAY_KEYSTORE_FILE").orNull
	signingConfigs {
		if (keystore != null) {
			create("release") {
				storeFile = file(keystore)
				storePassword = providers.environmentVariable("RELAY_KEYSTORE_PASSWORD").get()
				keyAlias = providers.environmentVariable("RELAY_KEY_ALIAS").get()
				keyPassword = providers.environmentVariable("RELAY_KEY_PASSWORD").get()
			}
		}
	}

	buildTypes {
		// The APK that ships is built by assembleDebug (debug key) but must not be debuggable:
		// a debuggable app lets any adb or run-as user read its private files. -Prelay.debuggable=true
		// is for development builds only; scripts/packaging/check_android_apk.py refuses such an APK.
		debug {
			isDebuggable = providers.gradleProperty("relay.debuggable").map { it == "true" }.orElse(false).get()
			isMinifyEnabled = false
		}
		release {
			isMinifyEnabled = false
			isDebuggable = false
			signingConfigs.findByName("release")?.let { signingConfig = it }
		}
	}

	sourceSets {
		getByName("main") {
			jniLibs.directories.add(jniOut.get().asFile.path)
			kotlin.directories.add(bindingsOut.get().asFile.path)
		}
	}

	packaging { jniLibs { useLegacyPackaging = false } }

	testOptions { unitTests.isReturnDefaultValues = true }
}

dependencies {
	implementation("androidx.games:games-activity:4.4.0")
	implementation("androidx.appcompat:appcompat:1.7.1")
	implementation("androidx.core:core-ktx:1.15.0")
	implementation("net.java.dev.jna:jna:5.19.1@aar")
	// QR scanning: CameraX preview plus ZXing's pure-Java decoder. No Google components:
	// scripts/packaging/check_android_apk.py fails the build if ML Kit, GMS or telemetry comes back.
	implementation("androidx.camera:camera-core:1.4.2")
	implementation("androidx.camera:camera-camera2:1.4.2")
	implementation("androidx.camera:camera-lifecycle:1.4.2")
	implementation("androidx.camera:camera-view:1.4.2")
	implementation("com.google.zxing:core:3.5.3")
	testImplementation("junit:junit:4.13.2")
	androidTestImplementation("androidx.test:runner:1.6.2")
	androidTestImplementation("androidx.test.ext:junit:1.2.1")
}

// One cargo-ndk build per ABI, straight into jniLibs.
val cargoNdk = tasks.register<Exec>("cargoNdk") {
	group = "relay"
	description = "Builds librelay_android.so for ${abis.joinToString()} with cargo-ndk."
	workingDir = rustDir
	environment("CARGO_TARGET_DIR", cargoTarget.get())
	environment("CARGO_INCREMENTAL", "0")
	val args = mutableListOf("cargo", "ndk")
	abis.forEach { args += listOf("-t", it) }
	args += listOf("-P", minSdkLevel.toString(), "-o", jniOut.get().asFile.path, "build", "--release")
	commandLine(args)
	inputs.files(fileTree(rustDir) { include("src/**", "third_party/**", "Cargo.toml", "Cargo.lock") })
	inputs.dir(rustDir.resolve("../../crates"))
	outputs.dir(jniOut)
}

// Kotlin bindings come from the built library, so Kotlin and Rust cannot drift.
val bindgen = tasks.register<Exec>("uniffiBindgen") {
	group = "relay"
	description = "Generates the Kotlin UniFFI bindings from the built library."
	dependsOn(cargoNdk)
	workingDir = rustDir.resolve("uniffi-bindgen")
	val hostTarget = providers.environmentVariable("RELAY_HOST_TARGET_DIR")
		.orElse(layout.buildDirectory.dir("cargo-host").map { it.asFile.absolutePath })
	environment("CARGO_TARGET_DIR", hostTarget.get())
	environment("CARGO_INCREMENTAL", "0")
	val library = jniOut.get().asFile.resolve("${abis.first()}/librelay_android.so")
	commandLine(
		"cargo", "run", "--release", "--",
		"generate", "--library", library.path, "--language", "kotlin",
		"--no-format", "--out-dir", bindingsOut.get().asFile.path,
	)
	// Regenerate whenever the library changes; without an input Gradle never reruns this.
	inputs.file(library)
	outputs.dir(bindingsOut)
}

tasks.named("preBuild") { dependsOn(bindgen) }
