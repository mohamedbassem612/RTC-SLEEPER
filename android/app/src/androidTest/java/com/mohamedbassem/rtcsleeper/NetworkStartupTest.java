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
            if (state.equals("NeedsLogin") || state.equals("Running")) {
                checkTimeoutAndRecovery(activity, instrumentation);
                return;
            }
            Thread.sleep(1000);
        }
        fail("Embedded node did not reach login/connected state: " + state);
    }

    private void checkTimeoutAndRecovery(MainActivity activity, android.app.Instrumentation instrumentation) throws Exception {
        // A real tsnet dial while not enrolled must honor its context deadline.
        java.util.concurrent.ExecutorService probe = java.util.concurrent.Executors.newSingleThreadExecutor();
        try {
            String result=probe.submit(() -> activity.nativeDialDeadlineTest()).get(5,java.util.concurrent.TimeUnit.SECONDS);
            org.json.JSONObject deadline=new org.json.JSONObject(result);
            assertTrue(deadline.toString(),deadline.getLong("elapsed_ms")<3000);
            assertFalse("Unavailable node should not connect",deadline.getString("error").isEmpty());
        } finally {probe.shutdownNow();}

        // A loopback-only debug laptop never contacts the real scheduler.
        java.net.ServerSocket listener=new java.net.ServerSocket(0,20,java.net.InetAddress.getByName("127.0.0.1"));
        java.util.concurrent.atomic.AtomicBoolean running=new java.util.concurrent.atomic.AtomicBoolean(true);
        java.util.concurrent.atomic.AtomicBoolean stalled=new java.util.concurrent.atomic.AtomicBoolean(false);
        Thread server=new Thread(() -> {
            while(running.get()) {
                try {
                    java.net.Socket socket=listener.accept();
                    Thread client=new Thread(() -> {
                        try(java.net.Socket connection=socket) {
                            new java.io.BufferedReader(new java.io.InputStreamReader(connection.getInputStream())).readLine();
                            while(stalled.get() && running.get()) Thread.sleep(30);
                            connection.getOutputStream().write("{\"ok\":true,\"result\":{\"status\":\"idle\"}}\n".getBytes(java.nio.charset.StandardCharsets.UTF_8));
                        } catch(Exception ignored) {}
                    });
                    client.setDaemon(true);client.start();
                } catch(Exception ignored) {break;}
            }
        });
        server.setDaemon(true);server.start();
        try {
            String session=new org.json.JSONObject().put("ip","127.0.0.1").put("port",listener.getLocalPort())
                .put("token","test-only").put("device","Instrumented fake laptop").toString();
            instrumentation.runOnMainSync(() -> activity.onNewIntent(new Intent().putExtra("demo_session",session)));
            awaitConnection(activity,false);
            stalled.set(true);
            awaitConnection(activity,true);
            stalled.set(false);
            awaitConnection(activity,false);
        } finally {running.set(false);stalled.set(false);listener.close();}
    }

    private void awaitConnection(MainActivity activity, boolean expectFailure) throws Exception {
        String last="";
        for(int i=0;i<250;i++) {
            last=activity.nativeConnectionTestState();
            if(!last.isEmpty()) {
                org.json.JSONObject state=new org.json.JSONObject(last);
                if(!state.getBoolean("busy") && "idle".equals(state.optString("status"))) {
                    if(expectFailure && state.getString("error").contains("timed out")) return;
                    if(!expectFailure && state.getString("error").isEmpty()) return;
                }
            }
            Thread.sleep(100);
        }
        fail("Connection did not " + (expectFailure?"time out":"recover") + ": " + last);
    }
}
