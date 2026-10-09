package kz.webnet.onlygaz.build;

import java.awt.color.ColorSpace;
import java.awt.image.BufferedImage;
import java.awt.image.ColorModel;
import java.awt.image.ComponentColorModel;
import java.awt.image.Raster;
import java.awt.image.WritableRaster;
import java.io.File;
import java.io.IOException;
import java.nio.file.FileVisitResult;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.SimpleFileVisitor;
import java.nio.file.StandardCopyOption;
import java.nio.file.attribute.BasicFileAttributes;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.function.Consumer;
import java.util.stream.Collectors;
import java.util.stream.Stream;
import javax.imageio.IIOImage;
import javax.imageio.ImageIO;
import javax.imageio.ImageWriteParam;
import javax.imageio.ImageWriter;
import javax.imageio.stream.FileImageOutputStream;

/**
 * Copies the game's asset tree for the Android build, sized for phone GPUs.
 *
 * <p>The desktop build ships 2K-12K textures. Phones have a fraction of the
 * memory bandwidth and share their RAM with the GPU, so the Android copy uses
 * smaller versions of the largest images. File names, formats and folder
 * structure stay identical, so the glTF scenes and the game code load the same
 * paths. The original {@code assets/} folder is never modified.
 *
 * <p>An optional overrides folder ({@code android/asset-overrides}) is laid on
 * top: a file there replaces the file with the same relative path. It holds
 * the simplified 3D models of the phone build, made by
 * {@code tools/make_mobile_models.py}.
 *
 * <p>Used by the Gradle build ({@link PrepareGameAssetsTask}) and, as a plain
 * command-line tool, by {@code android/build_apk.sh}:
 * {@code java kz.webnet.onlygaz.build.GameAssets <assets dir> <destination> [overrides dir]}.
 * Plain JDK only: no Gradle, Python or image libraries are needed.
 */
public final class GameAssets {
    /** No downscaling for the folder. */
    public static final int UNLIMITED = Integer.MAX_VALUE;
    /** Longest edge for files outside the listed folders. */
    public static final int DEFAULT_LIMIT = 1024;

    // Longest edge allowed per folder prefix (first match wins). Budget phones
    // have 3-4 GB of RAM shared by the system, the CPU and the GPU, and the
    // game keeps every texture resident, uncompressed, with its mipmaps.
    private static final String[] PREFIXES = {
        // The player's car is close to the camera, but drawn at most at about
        // a thousand pixels wide on a phone.
        "models/",
        // Logos and on-screen icons are drawn 1:1 by the interface.
        "ui/",
        "fonts/",
        // Day/night cubemaps are 1x6 strips: 512-px faces on a phone.
        "environment/skyboxes/",
        // The road surface fills the lower half of the screen.
        "environment/asphalt/",
        // Distant scenery and traffic.
        "environment/fir_trees/",
        "environment/",
        "traffic/",
        "effects/",
    };
    private static final int[] LIMITS = {1024, UNLIMITED, UNLIMITED, 3072, 1024, 512, 1024, 512, 512};

    private GameAssets() {}

    /** Longest edge allowed for an asset path relative to the assets folder. */
    public static int limitFor(String relativePath) {
        for (int i = 0; i < PREFIXES.length; i++) {
            if (relativePath.startsWith(PREFIXES[i])) {
                return LIMITS[i];
            }
        }
        return DEFAULT_LIMIT;
    }

    /** Read-me files and hidden files (.DS_Store, ._*) are not game data. */
    static boolean skipped(String relativePath) {
        if (relativePath.toLowerCase(Locale.ROOT).endsWith(".md")) {
            return true;
        }
        for (String segment : relativePath.split("/")) {
            if (segment.startsWith(".")) {
                return true;
            }
        }
        return false;
    }

    static boolean isImage(String relativePath) {
        String lower = relativePath.toLowerCase(Locale.ROOT);
        return lower.endsWith(".png") || lower.endsWith(".jpg") || lower.endsWith(".jpeg");
    }

    static boolean isJpeg(String relativePath) {
        String lower = relativePath.toLowerCase(Locale.ROOT);
        return lower.endsWith(".jpg") || lower.endsWith(".jpeg");
    }

    /** What happened to the assets, for the build log. */
    public static final class Summary {
        public int copied;
        public int resized;
        public int overridden;
        public long savedBytes;
        public long totalBytes;
        public final List<String> details = new ArrayList<>();
        public final List<String> warnings = new ArrayList<>();

        @Override
        public String toString() {
            return String.format(
                Locale.ROOT,
                "assets: %d copied, %d downscaled, %d phone versions, %.0f MB saved, %.0f MB total",
                copied,
                resized,
                overridden,
                savedBytes / 1e6,
                totalBytes / 1e6);
        }
    }

    private static final class Outcome {
        String detail;
        String warning;
        boolean resized;
        long savedBytes;
        long bytes;
    }

    /**
     * Rebuild {@code destination} from {@code source}. The destination folder
     * is deleted first, so it never keeps files that left the game.
     */
    public static Summary prepare(Path source, Path destination, int threads) throws IOException {
        return prepare(source, null, destination, threads);
    }

    /**
     * Rebuild {@code destination} from {@code source}, with the files of
     * {@code overrides} (may be {@code null}) replacing the files with the same
     * relative path. Overriding images are downscaled like the originals.
     */
    public static Summary prepare(Path source, Path overrides, Path destination, int threads)
            throws IOException {
        Path sourceRoot = source.toAbsolutePath().normalize();
        Path destinationRoot = destination.toAbsolutePath().normalize();
        if (!Files.isDirectory(sourceRoot)) {
            throw new IOException("asset folder not found: " + sourceRoot);
        }
        if (destinationRoot.startsWith(sourceRoot)) {
            throw new IOException("the destination must be outside the asset folder: " + destinationRoot);
        }
        Path overrideRoot = null;
        if (overrides != null) {
            overrideRoot = overrides.toAbsolutePath().normalize();
            if (!Files.isDirectory(overrideRoot)) {
                throw new IOException("asset overrides folder not found: " + overrideRoot);
            }
            if (destinationRoot.startsWith(overrideRoot)) {
                throw new IOException(
                    "the destination must be outside the overrides folder: " + destinationRoot);
            }
        }
        deleteRecursively(destinationRoot);
        Files.createDirectories(destinationRoot);

        // Relative path -> file to use, in a stable order.
        Map<String, Path> files = new TreeMap<>();
        collect(sourceRoot, files);
        Set<String> replaced = new HashSet<>();
        if (overrideRoot != null) {
            Map<String, Path> phoneVersions = new TreeMap<>();
            collect(overrideRoot, phoneVersions);
            for (Map.Entry<String, Path> entry : phoneVersions.entrySet()) {
                files.put(entry.getKey(), entry.getValue());
                replaced.add(entry.getKey());
            }
        }

        ExecutorService pool = Executors.newFixedThreadPool(Math.max(1, threads));
        List<Future<Outcome>> futures = new ArrayList<>();
        try {
            for (Map.Entry<String, Path> entry : files.entrySet()) {
                String relative = entry.getKey();
                Path file = entry.getValue();
                Path target = destinationRoot.resolve(relative);
                futures.add(pool.submit(() -> process(file, target, relative)));
            }
            Summary summary = new Summary();
            summary.overridden = replaced.size();
            for (Future<Outcome> future : futures) {
                Outcome outcome;
                try {
                    outcome = future.get();
                } catch (ExecutionException e) {
                    Throwable cause = e.getCause();
                    if (cause instanceof IOException) {
                        throw (IOException) cause;
                    }
                    throw new IOException(cause);
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    throw new IOException("interrupted", e);
                }
                if (outcome.resized) {
                    summary.resized++;
                    summary.savedBytes += outcome.savedBytes;
                    summary.details.add(outcome.detail);
                } else {
                    summary.copied++;
                }
                if (outcome.warning != null) {
                    summary.warnings.add(outcome.warning);
                }
                summary.totalBytes += outcome.bytes;
            }
            return summary;
        } finally {
            pool.shutdownNow();
        }
    }

    /** Add every game file under {@code root} to {@code files}, keyed by relative path. */
    private static void collect(Path root, Map<String, Path> files) throws IOException {
        try (Stream<Path> walk = Files.walk(root)) {
            for (Path file : walk.filter(Files::isRegularFile).collect(Collectors.toList())) {
                String relative = root.relativize(file).toString().replace(File.separatorChar, '/');
                if (!skipped(relative)) {
                    files.put(relative, file);
                }
            }
        }
    }

    private static Outcome process(Path file, Path target, String relative) throws IOException {
        Files.createDirectories(target.getParent());
        Outcome outcome = new Outcome();
        int limit = limitFor(relative);
        if (isImage(relative) && limit != UNLIMITED) {
            try {
                int[] sizes = downscale(file, target, limit, isJpeg(relative));
                if (sizes != null) {
                    long before = Files.size(file);
                    long after = Files.size(target);
                    outcome.resized = true;
                    outcome.savedBytes = before - after;
                    outcome.bytes = after;
                    outcome.detail = String.format(
                        Locale.ROOT, "%s: %dx%d -> %dx%d", relative, sizes[0], sizes[1], sizes[2], sizes[3]);
                    return outcome;
                }
            } catch (IOException | RuntimeException e) {
                // An image the JDK cannot decode still works at full size.
                outcome.warning = "kept " + relative + " at full size: " + e;
            }
        }
        Files.copy(file, target, StandardCopyOption.REPLACE_EXISTING);
        outcome.bytes = Files.size(target);
        return outcome;
    }

    /**
     * Write a smaller copy of {@code file} to {@code target}. Returns the old
     * and new sizes, or {@code null} when the image is already small enough.
     */
    static int[] downscale(Path file, Path target, int limit, boolean jpeg) throws IOException {
        BufferedImage image = ImageIO.read(file.toFile());
        if (image == null) {
            throw new IOException("unsupported image format");
        }
        int width = image.getWidth();
        int height = image.getHeight();
        int longest = Math.max(width, height);
        if (longest <= limit) {
            return null;
        }
        double scale = (double) limit / longest;
        int targetWidth = Math.max(1, (int) Math.round(width * scale));
        int targetHeight = Math.max(1, (int) Math.round(height * scale));

        Pixels pixels = Pixels.read(image);
        image = null; // the decoded image can be large; let it go before resampling
        BufferedImage resized = pixels.resample(targetWidth, targetHeight, jpeg);
        write(resized, target, jpeg);
        return new int[] {width, height, targetWidth, targetHeight};
    }

    private static void write(BufferedImage image, Path target, boolean jpeg) throws IOException {
        Iterator<ImageWriter> writers = ImageIO.getImageWritersByFormatName(jpeg ? "jpeg" : "png");
        if (!writers.hasNext()) {
            throw new IOException("no image writer for " + (jpeg ? "JPEG" : "PNG"));
        }
        ImageWriter writer = writers.next();
        ImageWriteParam parameters = writer.getDefaultWriteParam();
        if (parameters.canWriteCompressed()) {
            parameters.setCompressionMode(ImageWriteParam.MODE_EXPLICIT);
            // JPEG: quality 90 %. PNG: 0 = strongest deflate level (lossless).
            parameters.setCompressionQuality(jpeg ? 0.9f : 0.0f);
        }
        Files.deleteIfExists(target);
        try (FileImageOutputStream output = new FileImageOutputStream(target.toFile())) {
            writer.setOutput(output);
            writer.write(null, new IIOImage(image, null, null), parameters);
        } finally {
            writer.dispose();
        }
    }

    /**
     * 8-bit premultiplied ARGB pixels. Resampling premultiplied colours keeps
     * the transparent background of foliage billboards from bleeding into the
     * visible edges.
     */
    private static final class Pixels {
        final int width;
        final int height;
        final int[] premultiplied;
        final boolean alpha;
        final boolean gray;

        private Pixels(int width, int height, int[] premultiplied, boolean alpha, boolean gray) {
            this.width = width;
            this.height = height;
            this.premultiplied = premultiplied;
            this.alpha = alpha;
            this.gray = gray;
        }

        static Pixels read(BufferedImage image) {
            int width = image.getWidth();
            int height = image.getHeight();
            ColorModel model = image.getColorModel();
            boolean alpha = model.hasAlpha();
            boolean gray = model instanceof ComponentColorModel
                && model.getColorSpace().getType() == ColorSpace.TYPE_GRAY;
            int[] out = new int[width * height];
            if (gray) {
                // Raw samples: going through getRGB() would apply Java's linear
                // gray colour space and change the stored values.
                Raster raster = image.getRaster();
                int bands = raster.getNumBands();
                int grayBits = model.getComponentSize(0);
                int alphaBits = bands > 1 ? model.getComponentSize(1) : 8;
                int[] row = new int[width * bands];
                for (int y = 0; y < height; y++) {
                    raster.getPixels(0, y, width, 1, row);
                    for (int x = 0; x < width; x++) {
                        int value = to8Bit(row[x * bands], grayBits);
                        int a = bands > 1 ? to8Bit(row[x * bands + 1], alphaBits) : 255;
                        out[y * width + x] = premultiply(a, value, value, value);
                    }
                }
            } else {
                // Palette, RGB(A), 16-bit and 1-bit images, converted to sRGB ARGB.
                int[] row = new int[width];
                for (int y = 0; y < height; y++) {
                    image.getRGB(0, y, width, 1, row, 0, width);
                    for (int x = 0; x < width; x++) {
                        int argb = row[x];
                        out[y * width + x] = premultiply(
                            argb >>> 24, (argb >>> 16) & 0xff, (argb >>> 8) & 0xff, argb & 0xff);
                    }
                }
            }
            return new Pixels(width, height, out, alpha, gray);
        }

        private static int to8Bit(int value, int bits) {
            if (bits == 8) {
                return value;
            }
            int max = (1 << bits) - 1;
            return (int) Math.round(value * 255.0 / max);
        }

        private static int premultiply(int a, int r, int g, int b) {
            if (a == 255) {
                return 0xff000000 | (r << 16) | (g << 8) | b;
            }
            return (a << 24)
                | (((r * a + 127) / 255) << 16)
                | (((g * a + 127) / 255) << 8)
                | ((b * a + 127) / 255);
        }

        /**
         * Area-averaging resample (every output pixel is the exact average of
         * the source area it covers: a box filter, exact for the power-of-two
         * reductions the textures use) into an image of the original kind.
         */
        BufferedImage resample(int targetWidth, int targetHeight, boolean jpeg) {
            Span[] columns = Span.table(width, targetWidth);
            Span[] rows = Span.table(height, targetHeight);

            // Horizontal pass: width -> targetWidth for every source row.
            float[] middle = new float[targetWidth * height * 4];
            for (int y = 0; y < height; y++) {
                int rowStart = y * width;
                for (int x = 0; x < targetWidth; x++) {
                    Span span = columns[x];
                    float a = 0;
                    float r = 0;
                    float g = 0;
                    float b = 0;
                    for (int i = 0; i < span.weights.length; i++) {
                        int pixel = premultiplied[rowStart + span.first + i];
                        float weight = span.weights[i];
                        a += (pixel >>> 24) * weight;
                        r += ((pixel >>> 16) & 0xff) * weight;
                        g += ((pixel >>> 8) & 0xff) * weight;
                        b += (pixel & 0xff) * weight;
                    }
                    int o = (y * targetWidth + x) * 4;
                    middle[o] = a;
                    middle[o + 1] = r;
                    middle[o + 2] = g;
                    middle[o + 3] = b;
                }
            }

            boolean keepAlpha = alpha && !jpeg;
            int type;
            if (gray && !keepAlpha) {
                type = BufferedImage.TYPE_BYTE_GRAY;
            } else if (keepAlpha) {
                type = BufferedImage.TYPE_INT_ARGB;
            } else {
                type = BufferedImage.TYPE_INT_RGB;
            }
            BufferedImage output = new BufferedImage(targetWidth, targetHeight, type);
            WritableRaster grayRaster = type == BufferedImage.TYPE_BYTE_GRAY ? output.getRaster() : null;

            // Vertical pass: height -> targetHeight, then back to straight alpha.
            int[] line = new int[targetWidth];
            int[] grayLine = new int[targetWidth];
            for (int y = 0; y < targetHeight; y++) {
                Span span = rows[y];
                for (int x = 0; x < targetWidth; x++) {
                    float a = 0;
                    float r = 0;
                    float g = 0;
                    float b = 0;
                    for (int i = 0; i < span.weights.length; i++) {
                        int o = ((span.first + i) * targetWidth + x) * 4;
                        float weight = span.weights[i];
                        a += middle[o] * weight;
                        r += middle[o + 1] * weight;
                        g += middle[o + 2] * weight;
                        b += middle[o + 3] * weight;
                    }
                    int alphaValue = clamp(Math.round(a));
                    int red;
                    int green;
                    int blue;
                    if (a <= 0.5f) {
                        red = 0;
                        green = 0;
                        blue = 0;
                    } else if (alphaValue == 255) {
                        red = clamp(Math.round(r));
                        green = clamp(Math.round(g));
                        blue = clamp(Math.round(b));
                    } else {
                        float unpremultiply = 255f / a;
                        red = clamp(Math.round(r * unpremultiply));
                        green = clamp(Math.round(g * unpremultiply));
                        blue = clamp(Math.round(b * unpremultiply));
                    }
                    if (!keepAlpha) {
                        alphaValue = 255;
                    }
                    line[x] = (alphaValue << 24) | (red << 16) | (green << 8) | blue;
                    grayLine[x] = red;
                }
                if (grayRaster != null) {
                    grayRaster.setPixels(0, y, targetWidth, 1, grayLine);
                } else {
                    output.setRGB(0, y, targetWidth, 1, line, 0, targetWidth);
                }
            }
            return output;
        }

        private static int clamp(int value) {
            return Math.max(0, Math.min(255, value));
        }
    }

    /** Source pixels covered by one output pixel and their area weights. */
    private static final class Span {
        final int first;
        final float[] weights;

        private Span(int first, float[] weights) {
            this.first = first;
            this.weights = weights;
        }

        static Span[] table(int sourceLength, int targetLength) {
            double scale = (double) sourceLength / targetLength;
            Span[] spans = new Span[targetLength];
            for (int i = 0; i < targetLength; i++) {
                double start = i * scale;
                double end = Math.min(sourceLength, (i + 1) * scale);
                // The epsilon keeps rounding noise from adding a zero-weight pixel.
                int first = (int) Math.floor(start + 1e-9);
                int last = Math.min(sourceLength - 1, (int) Math.ceil(end - 1e-9) - 1);
                if (last < first) {
                    last = first;
                }
                float[] weights = new float[last - first + 1];
                double total = 0;
                for (int j = first; j <= last; j++) {
                    double covered = Math.min(end, j + 1) - Math.max(start, j);
                    weights[j - first] = (float) Math.max(0, covered);
                    total += Math.max(0, covered);
                }
                for (int j = 0; j < weights.length; j++) {
                    weights[j] = total > 0 ? (float) (weights[j] / total) : 1f / weights.length;
                }
                spans[i] = new Span(first, weights);
            }
            return spans;
        }
    }

    static void deleteRecursively(Path root) throws IOException {
        if (!Files.exists(root)) {
            return;
        }
        Files.walkFileTree(root, new SimpleFileVisitor<Path>() {
            @Override
            public FileVisitResult visitFile(Path file, BasicFileAttributes attributes) throws IOException {
                Files.delete(file);
                return FileVisitResult.CONTINUE;
            }

            @Override
            public FileVisitResult postVisitDirectory(Path directory, IOException error) throws IOException {
                if (error != null) {
                    throw error;
                }
                Files.delete(directory);
                return FileVisitResult.CONTINUE;
            }
        });
    }

    /** Command line: {@code GameAssets <assets dir> <destination dir> [overrides dir]}. */
    public static void main(String[] arguments) throws IOException {
        if (arguments.length != 2 && arguments.length != 3) {
            System.err.println(
                "usage: java kz.webnet.onlygaz.build.GameAssets <assets dir> <destination dir> [overrides dir]");
            System.exit(2);
        }
        System.setProperty("java.awt.headless", "true");
        int threads = Math.min(4, Runtime.getRuntime().availableProcessors());
        Path overrides = arguments.length == 3 ? Path.of(arguments[2]) : null;
        Summary summary = prepare(Path.of(arguments[0]), overrides, Path.of(arguments[1]), threads);
        for (String detail : summary.details) {
            System.out.println("  " + detail);
        }
        for (String warning : summary.warnings) {
            System.out.println("  warning: " + warning);
        }
        System.out.println(summary);
    }
}
