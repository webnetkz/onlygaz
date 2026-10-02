package kz.webnet.onlygaz.build;

import java.io.File;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/** Android ABIs and NDK tools needed to build the Rust library. Plain JDK only. */
public final class AndroidToolchain {
    private AndroidToolchain() {}

    /** An Android ABI with its Rust target and NDK clang target names. */
    public static final class Abi {
        public final String name;
        public final String rustTarget;
        public final String clangTarget;

        private Abi(String name, String rustTarget, String clangTarget) {
            this.name = name;
            this.rustTarget = rustTarget;
            this.clangTarget = clangTarget;
        }

        /** ABI names as Android uses them. */
        public static final List<String> SUPPORTED = List.of("arm64-v8a", "x86_64", "armeabi-v7a", "x86");

        public static Abi of(String name) {
            switch (name) {
                case "arm64-v8a":
                    return new Abi(name, "aarch64-linux-android", "aarch64-linux-android");
                case "x86_64":
                    return new Abi(name, "x86_64-linux-android", "x86_64-linux-android");
                case "armeabi-v7a":
                    return new Abi(name, "armv7-linux-androideabi", "armv7a-linux-androideabi");
                case "x86":
                    return new Abi(name, "i686-linux-android", "i686-linux-android");
                default:
                    throw new IllegalArgumentException(
                        "unsupported Android ABI '" + name + "', expected one of " + SUPPORTED);
            }
        }

        /** Cargo's environment-variable spelling of the Rust target. */
        public String rustTargetEnv() {
            return rustTarget.toUpperCase(Locale.ROOT).replace('-', '_');
        }
    }

    public static boolean windows() {
        return System.getProperty("os.name", "").toLowerCase(Locale.ROOT).startsWith("windows");
    }

    /**
     * NDK host folder. macOS NDKs (Intel and Apple Silicon) use
     * {@code darwin-x86_64} for their universal binaries.
     */
    public static String hostTag() {
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        if (os.startsWith("mac") || os.contains("darwin")) {
            return "darwin-x86_64";
        }
        if (os.startsWith("windows")) {
            return "windows-x86_64";
        }
        return "linux-x86_64";
    }

    /**
     * Locate the NDK: an explicit folder, {@code <sdk>/ndk/<version>},
     * {@code ANDROID_NDK_HOME} / {@code ANDROID_NDK_ROOT}, the newest
     * {@code <sdk>/ndk/*}, then the legacy {@code <sdk>/ndk-bundle}.
     *
     * @return the NDK folder, or {@code null} when none is installed
     */
    public static File findNdk(String explicit, String sdkDirectory, String version, Map<String, String> environment) {
        if (explicit != null && !explicit.isBlank()) {
            return new File(explicit);
        }
        File sdk = sdkDirectory == null || sdkDirectory.isBlank() ? null : new File(sdkDirectory);
        if (sdk != null && version != null && !version.isBlank()) {
            File exact = new File(sdk, "ndk/" + version);
            if (isNdk(exact)) {
                return exact;
            }
        }
        for (String variable : List.of("ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "NDK_HOME")) {
            String value = environment.get(variable);
            if (value != null && !value.isBlank() && isNdk(new File(value))) {
                return new File(value);
            }
        }
        if (sdk != null) {
            File[] installed = new File(sdk, "ndk").listFiles(AndroidToolchain::isNdk);
            if (installed != null && installed.length > 0) {
                return Arrays.stream(installed)
                    .max(Comparator.comparing(File::getName, AndroidToolchain::compareVersions))
                    .get();
            }
            File bundle = new File(sdk, "ndk-bundle");
            if (isNdk(bundle)) {
                return bundle;
            }
        }
        return null;
    }

    static boolean isNdk(File directory) {
        return new File(directory, "toolchains/llvm/prebuilt").isDirectory();
    }

    /** {@code <ndk>/toolchains/llvm/prebuilt/<host>/bin}. */
    public static File toolchainBin(File ndk) {
        File prebuilt = new File(ndk, "toolchains/llvm/prebuilt");
        File host = new File(prebuilt, hostTag());
        if (!host.isDirectory()) {
            File[] hosts = prebuilt.listFiles(File::isDirectory);
            if (hosts != null && hosts.length == 1) {
                host = hosts[0];
            }
        }
        return new File(host, "bin");
    }

    /** NDK clang for a target and API level, e.g. {@code aarch64-linux-android26-clang}. */
    public static File clang(File bin, Abi abi, int apiLevel, boolean cxx) {
        String name = abi.clangTarget + apiLevel + (cxx ? "-clang++" : "-clang");
        return new File(bin, windows() ? name + ".cmd" : name);
    }

    public static File llvmAr(File bin) {
        return new File(bin, windows() ? "llvm-ar.exe" : "llvm-ar");
    }

    /** Numeric comparison of dotted versions such as {@code 27.0.12077973}. */
    static int compareVersions(String left, String right) {
        String[] a = left.split("[.-]");
        String[] b = right.split("[.-]");
        for (int i = 0; i < Math.max(a.length, b.length); i++) {
            long x = i < a.length ? parse(a[i]) : 0;
            long y = i < b.length ? parse(b[i]) : 0;
            if (x != y) {
                return Long.compare(x, y);
            }
        }
        return 0;
    }

    private static long parse(String part) {
        try {
            return Long.parseLong(part);
        } catch (NumberFormatException e) {
            return -1;
        }
    }
}
