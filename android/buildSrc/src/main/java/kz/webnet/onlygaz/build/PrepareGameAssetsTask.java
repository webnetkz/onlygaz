package kz.webnet.onlygaz.build;

import java.io.IOException;
import java.nio.file.Path;
import org.gradle.api.DefaultTask;
import org.gradle.api.file.DirectoryProperty;
import org.gradle.api.tasks.InputDirectory;
import org.gradle.api.tasks.Optional;
import org.gradle.api.tasks.OutputDirectory;
import org.gradle.api.tasks.PathSensitive;
import org.gradle.api.tasks.PathSensitivity;
import org.gradle.api.tasks.TaskAction;

/**
 * Builds the phone-sized copy of the repository's {@code assets/} folder that
 * goes into the APK / app bundle (see {@link GameAssets}). Gradle re-runs it
 * only when a file in {@code assets/}, in the overrides folder or this code
 * changes.
 */
public abstract class PrepareGameAssetsTask extends DefaultTask {
    /** The game's assets folder (repository root {@code assets/}). */
    @InputDirectory
    @PathSensitive(PathSensitivity.RELATIVE)
    public abstract DirectoryProperty getSourceDirectory();

    /**
     * Phone versions of individual assets ({@code android/asset-overrides}),
     * laid over the copy of {@link #getSourceDirectory()}.
     */
    @InputDirectory
    @Optional
    @PathSensitive(PathSensitivity.RELATIVE)
    public abstract DirectoryProperty getOverrideDirectory();

    /** Generated assets folder handed to the Android build. */
    @OutputDirectory
    public abstract DirectoryProperty getOutputDirectory();

    @TaskAction
    public void prepare() throws IOException {
        int threads = Math.min(4, Runtime.getRuntime().availableProcessors());
        Path overrides = getOverrideDirectory().isPresent()
            ? getOverrideDirectory().get().getAsFile().toPath()
            : null;
        GameAssets.Summary summary = GameAssets.prepare(
            getSourceDirectory().get().getAsFile().toPath(),
            overrides,
            getOutputDirectory().get().getAsFile().toPath(),
            threads);
        for (String detail : summary.details) {
            getLogger().info("  {}", detail);
        }
        for (String warning : summary.warnings) {
            getLogger().warn("OnlyGAZ assets: {}", warning);
        }
        getLogger().lifecycle("OnlyGAZ {}", summary);
    }
}
