package kz.webnet.onlygaz;

import android.app.NativeActivity;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowManager;

/**
 * OnlyGAZ runs entirely in native code (Rust + Bevy, libonlygaz.so) through
 * {@link NativeActivity}. This subclass only adds what NativeActivity cannot
 * do by itself:
 *
 * <ul>
 *   <li>immersive full screen: status and navigation bars stay hidden and
 *       return only temporarily after a swipe from the screen edge;</li>
 *   <li>the screen stays on during a race;</li>
 *   <li>the safe-area insets of display cut-outs (camera holes, notches) are
 *       published in static fields that the game reads through JNI to keep
 *       its HUD and buttons clear of them;</li>
 *   <li>the display rotation is reported so tilt steering works in both
 *       landscape orientations.</li>
 * </ul>
 *
 * Gradle compiles it against API 36; android/build_apk.sh may compile the same
 * file against API 23 (no Google SDK download needed), so APIs newer than 23
 * are reached through reflection.
 */
// The pre-API-30 system UI calls are deliberate: they cover Android 8-10,
// and the newer WindowInsetsController is reached through reflection.
@SuppressWarnings("deprecation")
public class MainActivity extends NativeActivity {
    /** Cut-out safe insets in physical pixels (read by the native game). */
    public static volatile int safeInsetLeft;
    public static volatile int safeInsetTop;
    public static volatile int safeInsetRight;
    public static volatile int safeInsetBottom;

    private static final int IMMERSIVE_FLAGS =
            View.SYSTEM_UI_FLAG_LAYOUT_STABLE
            | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
            | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
            | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
            | View.SYSTEM_UI_FLAG_FULLSCREEN
            | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
                | WindowManager.LayoutParams.FLAG_FULLSCREEN);
        super.onCreate(savedInstanceState);

        View decor = getWindow().getDecorView();
        decor.setOnApplyWindowInsetsListener(new View.OnApplyWindowInsetsListener() {
            @Override
            public WindowInsets onApplyWindowInsets(View view, WindowInsets insets) {
                publishCutoutInsets(insets);
                return view.onApplyWindowInsets(insets);
            }
        });
        decor.setOnSystemUiVisibilityChangeListener(new View.OnSystemUiVisibilityChangeListener() {
            @Override
            public void onSystemUiVisibilityChange(int visibility) {
                if ((visibility & View.SYSTEM_UI_FLAG_FULLSCREEN) == 0) {
                    hideSystemBars();
                }
            }
        });
        hideSystemBars();
    }

    @Override
    protected void onResume() {
        super.onResume();
        hideSystemBars();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            hideSystemBars();
        }
    }

    /**
     * Display rotation (0..3 = 0°, 90°, 180°, 270°), polled by the game to
     * read the tilt sensor in screen axes. Turning a landscape phone over by
     * 180° does not change the configuration, hence polling.
     */
    public int currentDisplayRotation() {
        try {
            return getWindowManager().getDefaultDisplay().getRotation();
        } catch (Throwable ignored) {
            return 1;
        }
    }

    /** Back is handled by the game (pause / previous menu / quit). */
    @Override
    public void onBackPressed() {
    }

    private void hideSystemBars() {
        View decor = getWindow().getDecorView();
        decor.setSystemUiVisibility(IMMERSIVE_FLAGS);
        if (Build.VERSION.SDK_INT >= 30) {
            try {
                Object window = getWindow();
                Object controller = window.getClass().getMethod("getInsetsController").invoke(window);
                if (controller != null) {
                    int systemBars = (Integer) Class.forName("android.view.WindowInsets$Type")
                            .getMethod("systemBars").invoke(null);
                    controller.getClass().getMethod("hide", int.class).invoke(controller, systemBars);
                    // WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                    controller.getClass().getMethod("setSystemBarsBehavior", int.class)
                            .invoke(controller, 2);
                }
            } catch (Throwable ignored) {
                // setSystemUiVisibility above still applies.
            }
        }
    }

    private static void publishCutoutInsets(WindowInsets insets) {
        int left = 0, top = 0, right = 0, bottom = 0;
        if (Build.VERSION.SDK_INT >= 28 && insets != null) {
            try {
                Object cutout = WindowInsets.class.getMethod("getDisplayCutout").invoke(insets);
                if (cutout != null) {
                    Class<?> type = cutout.getClass();
                    left = (Integer) type.getMethod("getSafeInsetLeft").invoke(cutout);
                    top = (Integer) type.getMethod("getSafeInsetTop").invoke(cutout);
                    right = (Integer) type.getMethod("getSafeInsetRight").invoke(cutout);
                    bottom = (Integer) type.getMethod("getSafeInsetBottom").invoke(cutout);
                }
            } catch (Throwable ignored) {
                // Keep the zero insets.
            }
        }
        safeInsetLeft = left;
        safeInsetTop = top;
        safeInsetRight = right;
        safeInsetBottom = bottom;
    }
}
