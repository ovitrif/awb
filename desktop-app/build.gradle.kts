import org.jetbrains.compose.desktop.application.dsl.TargetFormat
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    kotlin("jvm")
    kotlin("plugin.serialization")
    id("org.jetbrains.compose")
    id("org.jetbrains.kotlin.plugin.compose")
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    implementation(compose.desktop.currentOs)
    implementation("org.jetbrains.compose.material3:material3:1.9.0")
    implementation("org.jetbrains.compose.material:material-icons-extended:1.7.3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
}

val cargoBuildRelease by tasks.registering(Exec::class) {
    workingDir = rootProject.projectDir
    commandLine("cargo", "build", "--release")
}

tasks.withType<JavaExec>().configureEach {
    dependsOn(cargoBuildRelease)
    environment("AIRADB_BIN", rootProject.layout.projectDirectory.file("target/release/airadb").asFile.absolutePath)
}

compose.desktop {
    application {
        mainClass = "com.ovitrif.airadb.desktop.MainKt"

        nativeDistributions {
            targetFormats(TargetFormat.Dmg, TargetFormat.Pkg)
            packageName = "airadb-desktop"
            packageVersion = "0.1.15"
            description = "Desktop utility for airadb Android wireless debugging."

            macOS {
                bundleID = "com.ovitrif.airadb.desktop"
                appCategory = "public.app-category.utilities"
            }
        }
    }
}
