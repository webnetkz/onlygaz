package kz.webnet.onlygaz.build;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import javax.inject.Inject;
import org.gradle.api.DefaultTask;
import org.gradle.api.GradleException;
import org.gradle.api.file.DirectoryProperty;
import org.gradle.api.provider.ListProperty;
import org.gradle.api.provider.MapProperty;
import org.gradle.api.provider.Property;
import org.gradle.api.tasks.Input;
import org.gradle.api.tasks.Internal;
import org.gradle.api.tasks.Optional;
import org.gradle.api.tasks.OutputDirectory;
import org.gradle.api.tasks.TaskAction;
import org.gradle.process.ExecOperations;
import org.gradle.process.ExecResult;

/**
 * Compiles the game's Rust library ({@code android/Cargo.toml}) with cargo and
 * the NDK's clang for each requested ABI and lays the results out as
 * {@code <output>/<abi>/libonlygaz.so} for the Android build.
 *
 * <p>The task always runs: cargo itself knows which Rust sources changed and
 * finishes in a second or two when nothing did.
 */
public abstract class CargoBuildTask extends DefaultTask {
    /** Folder with the Cargo.toml of the Android library ({@code android/}). */
    @Internal
    public abstract DirectoryProperty getCrateDirectory();

    /** Cargo profile: {@code release}, {@code android-dev}, ... */
    @Input
    public abstract Property<String> getCargoProfile();

    /** Android ABIs to build, e.g. {@code arm64-v8a}. */
    @Input
    public abstract ListProperty<String> getAbis();

    /** minSdk: selects the NDK's {@code <target><api>-clang} wrapper. */
    @Input
    public abstract Property<Integer> getApiLevel();

    /** Library name without {@code lib} and {@code .so}. */
    @Input
    public abstract Property<String> getLibraryName();

    /** Preferred NDK version ({@code android.ndkVersion}). */
    @Input
    @Optional
    public abstract Property<String> getNdkVersion();

    /** Android SDK folder ({@code sdk.dir} in local.properties). */
    @Input
    @Optional
    public abstract Property<String> getSdkDirectory();

    /** Explicit NDK folder ({@code onlygaz.ndkDir}), overrides the search. */
    @Input
    @Optional
    public abstract Property<String> getNdkDirectory();

    /** Explicit cargo binary ({@code onlygaz.cargo}). */
    @Input
    @Optional
    public abstract Property<String> getCargoExecutable();

    /** Extra cargo arguments ({@code onlygaz.cargoArgs}). */
    @Input
    public abstract ListProperty<String> getExtraCargoArguments();

    /** Extra environment for cargo ({@code onlygaz.cargoEnv}). */
    @Input
    public abstract MapProperty<String, String> getExtraEnvironment();

    /** Receives {@code <abi>/lib<name>.so}. */
    @OutputDirectory
    public abstract DirectoryProperty getOutputDirectory();

    @Inject
    protected abstract ExecOperations getExecOperations();

    public CargoBuildTask() {
        getLibraryName().convention("onlygaz");
        getExtraCargoArguments().convention(Collections.emptyList());
        getExtraEnvironment().convention(Collections.emptyMap());
        getOutputs().upToDateWhen(task -> false);
    }

    @TaskAction
    public void build() throws IOException {
        File crate = getCrateDirectory().get().getAsFile();
        String profile = getCargoProfile().get();
        List<String> abis = getAbis().get();
        if (abis.isEmpty()) {
            throw new GradleException("no Android ABI selected (onlygaz.abis)");
        }
        Map<String, String> environment = new HashMap<>(System.getenv());
        environment.putAll(getExtraEnvironment().get());

        File cargo = findCargo(getCargoExecutable().getOrNull(), environment);
        if (cargo == null) {
            throw new GradleException(
                "cargo not found. Install Rust with rustup (https://rustup.rs) or set "
                    + "onlygaz.cargo=/path/to/cargo in android/local.properties.");
        }
        File ndk = AndroidToolchain.findNdk(
            getNdkDirectory().getOrNull(), getSdkDirectory().getOrNull(), getNdkVersion().getOrNull(), environment);
        if (ndk == null || !ndk.isDirectory()) {
            throw new GradleException(
                "Android NDK not found. Install it in Android Studio: Settings > Languages & Frameworks > "
                    + "Android SDK > SDK Tools > NDK (Side by side)"
                    + (getNdkVersion().isPresent() ? " (version " + getNdkVersion().get() + ")" : "")
                    + ", or set onlygaz.ndkDir in android/local.properties.");
        }
        File bin = AndroidToolchain.toolchainBin(ndk);
        File ar = AndroidToolchain.llvmAr(bin);

        File output = getOutputDirectory().get().getAsFile();
        GameAssets.deleteRecursively(output.toPath());
        Files.createDirectories(output.toPath());

        File targetDirectory = cargoTargetDirectory(crate, environment);
        String profileDirectory = profileDirectory(profile);
        String library = "lib" + getLibraryName().get() + ".so";
        boolean buildStd = getExtraCargoArguments().get().stream().anyMatch(a -> a.startsWith("-Zbuild-std"));

        for (String abiName : abis) {
            AndroidToolchain.Abi abi = AndroidToolchain.Abi.of(abiName);
            File clang = AndroidToolchain.clang(bin, abi, getApiLevel().get(), false);
            File clangxx = AndroidToolchain.clang(bin, abi, getApiLevel().get(), true);
            if (!clang.isFile()) {
                throw new GradleException("NDK clang not found: " + clang + " (NDK " + ndk + ")");
            }
            if (!buildStd) {
                ensureRustTarget(cargo, crate, abi.rustTarget, environment);
            }

            List<String> arguments = new ArrayList<>(
                List.of("build", "--lib", "--target", abi.rustTarget, "--profile", profile));
            arguments.addAll(getExtraCargoArguments().get());
            getLogger().lifecycle(
                "OnlyGAZ: cargo {} ({} with NDK {})", String.join(" ", arguments), abi.name, ndk.getName());

            String underscored = abi.rustTarget.replace('-', '_');
            String path = cargo.getParent() + File.pathSeparator + environment.getOrDefault("PATH", "");
            ExecResult result = getExecOperations().exec(spec -> {
                spec.setWorkingDir(crate);
                spec.setExecutable(cargo.getAbsolutePath());
                spec.setArgs(arguments);
                spec.environment(getExtraEnvironment().get());
                spec.environment("PATH", path);
                spec.environment("CARGO_TARGET_" + abi.rustTargetEnv() + "_LINKER", clang.getAbsolutePath());
                spec.environment("CC_" + underscored, clang.getAbsolutePath());
                spec.environment("CXX_" + underscored, clangxx.getAbsolutePath());
                spec.environment("AR_" + underscored, ar.getAbsolutePath());
                spec.environment("ANDROID_NDK_HOME", ndk.getAbsolutePath());
                spec.environment("ANDROID_NDK_ROOT", ndk.getAbsolutePath());
                // Plain logs in Android Studio's Build window.
                spec.environment("CARGO_TERM_COLOR", "never");
                spec.environment("CARGO_TERM_PROGRESS_WHEN", "never");
                spec.setIgnoreExitValue(true);
            });
            if (result.getExitValue() != 0) {
                throw new GradleException(
                    "cargo failed for " + abi.name + " (exit code " + result.getExitValue()
                        + "); the Rust errors are above in the build output.");
            }

            File built = new File(targetDirectory, abi.rustTarget + "/" + profileDirectory + "/" + library);
            if (!built.isFile()) {
                throw new GradleException("cargo finished but " + built + " is missing");
            }
            File destination = new File(output, abi.name + "/" + library);
            Files.createDirectories(destination.getParentFile().toPath());
            Files.copy(built.toPath(), destination.toPath(), StandardCopyOption.REPLACE_EXISTING);
            getLogger().lifecycle(
                "OnlyGAZ: {} {} ({} MB)",
                abi.name,
                library,
                String.format(Locale.ROOT, "%.1f", destination.length() / 1e6));
        }
    }

    /** Install the Rust standard library for the target with rustup if needed. */
    private void ensureRustTarget(File cargo, File crate, String target, Map<String, String> environment) {
        File rustup = new File(cargo.getParentFile(), AndroidToolchain.windows() ? "rustup.exe" : "rustup");
        if (!rustup.isFile()) {
            // Rust without rustup: cargo reports a missing target itself.
            return;
        }
        ByteArrayOutputStream installed = new ByteArrayOutputStream();
        ExecResult list = getExecOperations().exec(spec -> {
            spec.setWorkingDir(crate);
            spec.setExecutable(rustup.getAbsolutePath());
            spec.args("target", "list", "--installed");
            spec.environment(getExtraEnvironment().get());
            spec.setStandardOutput(installed);
            spec.setIgnoreExitValue(true);
        });
        if (list.getExitValue() != 0) {
            return;
        }
        for (String line : installed.toString(StandardCharsets.UTF_8).split("\\R")) {
            if (line.trim().equals(target)) {
                return;
            }
        }
        getLogger().lifecycle("OnlyGAZ: installing the Rust target {} (rustup target add {})", target, target);
        ExecResult add = getExecOperations().exec(spec -> {
            spec.setWorkingDir(crate);
            spec.setExecutable(rustup.getAbsolutePath());
            spec.args("target", "add", target);
            spec.environment(getExtraEnvironment().get());
            spec.setIgnoreExitValue(true);
        });
        if (add.getExitValue() != 0) {
            throw new GradleException("could not install the Rust target " + target + ": run 'rustup target add "
                + target + "' in a terminal");
        }
    }

    static File findCargo(String explicit, Map<String, String> environment) {
        if (explicit != null && !explicit.isBlank()) {
            return new File(explicit);
        }
        String executable = AndroidToolchain.windows() ? "cargo.exe" : "cargo";
        // Android Studio started from the Dock does not see the shell's PATH,
        // so look in rustup's default location first.
        String cargoHome = environment.get("CARGO_HOME");
        if (cargoHome != null && !cargoHome.isBlank()) {
            File candidate = new File(cargoHome, "bin/" + executable);
            if (candidate.isFile()) {
                return candidate;
            }
        }
        File home = new File(System.getProperty("user.home"), ".cargo/bin/" + executable);
        if (home.isFile()) {
            return home;
        }
        String path = environment.get("PATH");
        if (path != null) {
            for (String directory : path.split(File.pathSeparator)) {
                File candidate = new File(directory, executable);
                if (!directory.isEmpty() && candidate.isFile()) {
                    return candidate;
                }
            }
        }
        return null;
    }

    static File cargoTargetDirectory(File crate, Map<String, String> environment) {
        String configured = environment.get("CARGO_TARGET_DIR");
        if (configured == null || configured.isBlank()) {
            return new File(crate, "target");
        }
        File directory = new File(configured);
        return directory.isAbsolute() ? directory : new File(crate, configured);
    }

    /** Folder cargo writes a profile's output to. */
    static String profileDirectory(String profile) {
        switch (profile) {
            case "dev":
            case "test":
                return "debug";
            case "bench":
                return "release";
            default:
                return profile;
        }
    }
}
