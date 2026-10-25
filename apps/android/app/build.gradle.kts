plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

abstract class BuildCore : Exec() {
    @get:OutputDirectory
    abstract val bindingsDir: DirectoryProperty
}

val buildCore = tasks.register<BuildCore>("buildCore") {
    description = "Builds spentDay-core for Android and generates its Kotlin bindings."
    bindingsDir.set(layout.buildDirectory.dir("generated/uniffi/kotlin"))
    val script = rootProject.file("build-core.sh")
    commandLine(script.absolutePath, bindingsDir.get().asFile.absolutePath)
    outputs.upToDateWhen { false }
}

// Release signing comes from ~/.gradle/gradle.properties or the environment, never from this repo.
fun signingValue(name: String): String? =
    providers.gradleProperty(name).orNull ?: providers.environmentVariable(name).orNull
val releaseKeystore = signingValue("SPENTDAY_KEYSTORE")

android {
    namespace = "day.spent.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "day.spent.android"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = signingValue("SPENTDAY_KEYSTORE_PASSWORD")
                keyAlias = signingValue("SPENTDAY_KEY_ALIAS") ?: "spentday"
                keyPassword = signingValue("SPENTDAY_KEY_PASSWORD") ?: storePassword
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.findByName("release")
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    testOptions {
        unitTests.isIncludeAndroidResources = true
        // Robolectric's SDK 36 sandbox reaches into FileDescriptor.
        unitTests.all {
            it.jvmArgs("--add-exports=java.base/jdk.internal.access=ALL-UNNAMED")
            // Tests that use the core load the host library build-core.sh builds.
            it.systemProperty("jna.library.path", rootProject.file("../../target/release").absolutePath)
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }
}

androidComponents {
    onVariants { variant ->
        variant.sources.kotlin?.addGeneratedSourceDirectory(buildCore, BuildCore::bindingsDir)
    }
}

tasks.named("preBuild") { dependsOn(buildCore) }

// Without a keystore the release APK would come out unsigned, and Android won't install it.
val checkReleaseSigning = tasks.register("checkReleaseSigning") {
    val missing = releaseKeystore == null
    doLast {
        if (missing) {
            throw GradleException(
                "Release builds need a keystore. Set SPENTDAY_KEYSTORE and SPENTDAY_KEYSTORE_PASSWORD " +
                    "in ~/.gradle/gradle.properties or the environment. See the Android section of README.md.",
            )
        }
    }
}
tasks.matching { it.name == "preReleaseBuild" }.configureEach { dependsOn(checkReleaseSigning) }

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.work.runtime)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons)
    implementation(libs.compose.ui.tooling.preview)
    debugImplementation(libs.compose.ui.tooling)
    implementation("${libs.jna.get()}@aar")
    testImplementation(libs.junit)
    testImplementation(libs.jna)
    testImplementation(libs.robolectric)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    debugImplementation(libs.compose.ui.test.manifest)
}
