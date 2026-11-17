package com.mohamedbassem.rtcsleeper;

import android.content.Intent;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;
import org.junit.Test;
import org.junit.runner.RunWith;
import static org.junit.Assert.*;

/** Starts the real embedded node in a normal app UID, not adb/root. No power commands. */
@RunWith(AndroidJUnit4.class)
public final class NetworkStartupTest {
    @Test public void androidSandboxCanStartEmbeddedTailscale() throws Exception {
        assertTrue("Run this regression on API 30+", android.os.Build.VERSION.SDK_INT >= 30);
        android.app.Instrumentation instrumentation = InstrumentationRegistry.getInstrumentation();
        Intent intent = new Intent(instrumentation.getTargetContext(), MainActivity.class)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        MainActivity activity = (MainActivity)instrumentation.startActivitySync(intent);
        org.json.JSONArray networks = new org.json.JSONArray(activity.networkSnapshot());
        assertTrue("Test emulator must have a connected network", networks.length() > 0);
        String state = "";
        for (int i=0; i<120; i++) {
            state = activity.nativeNetworkTestState();
            assertFalse("NETLINK_ROUTE regression: " + state, state.contains("netlinkrib"));
            assertFalse("Embedded node failed: " + state, state.startsWith("error:"));
            if (state.equals("NeedsLogin") || state.equals("Running")) return;
            Thread.sleep(1000);
        }
        fail("Embedded node did not reach login/connected state: " + state);
    }
}
