package com.mohamedbassem.rtcsleeper;

import android.app.NativeActivity;
import android.content.Intent;
import android.os.Bundle;
import android.net.Uri;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;
import com.google.zxing.integration.android.IntentIntegrator;
import com.google.zxing.integration.android.IntentResult;
import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

/** Android camera/Keystore adapter only. UI, protocol and transport are Rust. */
public final class MainActivity extends NativeActivity {
    static { System.loadLibrary("rtc_sleeper_android"); }
    private native void nativeScan(String code);
    // Implemented only in debug Rust binaries; no credentials are returned.
    public native String nativeNetworkTestState();
    public native String nativeConnectionTestState();
    public native String nativeDialDeadlineTest();
    @Override protected void onCreate(Bundle state) {
        try {
            android.system.Os.setenv("HOME", getFilesDir().getAbsolutePath(), true);
            android.system.Os.setenv("TMPDIR", getCacheDir().getAbsolutePath(), true);
        } catch (android.system.ErrnoException ex) {
            throw new IllegalStateException("Cannot initialize native private storage", ex);
        }
        super.onCreate(state);
        getWindow().setStatusBarColor(0xff000000);
        getWindow().setNavigationBarColor(0xff000000);
        debugInput(getIntent());
    }
    @Override protected void onNewIntent(Intent intent) { super.onNewIntent(intent); setIntent(intent); debugInput(intent); }
    private void debugInput(Intent intent) {
        // A debug-only, loopback-only route for emulator integration tests.
        if (BuildConfig.DEBUG && intent != null && intent.hasExtra("demo_pair"))
            nativeScan("demo:" + intent.getStringExtra("demo_pair"));
        if (BuildConfig.DEBUG && intent != null && intent.hasExtra("demo_session"))
            nativeScan("session:" + intent.getStringExtra("demo_session"));
    }
    public String scanQr() {
        runOnUiThread(() -> new IntentIntegrator(this)
            .setDesiredBarcodeFormats(IntentIntegrator.QR_CODE)
            .setPrompt("Scan the QR code in RTC Sleeper on your laptop")
            .setBeepEnabled(false).setOrientationLocked(false).initiateScan());
        return "";
    }
    public String enterPhrase() {
        runOnUiThread(() -> {
            android.widget.EditText entry = new android.widget.EditText(this);
            entry.setHint("24-word pairing phrase");
            entry.setMinLines(4);
            entry.setInputType(android.text.InputType.TYPE_CLASS_TEXT | android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE | android.text.InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD);
            entry.setImeOptions(android.view.inputmethod.EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING);
            new android.app.AlertDialog.Builder(this).setTitle("Pairing Phrase").setView(entry)
                .setPositiveButton("Use phrase", (dialog, which) -> nativeScan(entry.getText().toString().trim()))
                .setNegativeButton("Cancel", null).show();
        });
        return "";
    }
    @Override protected void onActivityResult(int request, int result, Intent data) {
        IntentResult scan = IntentIntegrator.parseActivityResult(request, result, data);
        if (scan != null) { if (scan.getContents() != null) nativeScan(scan.getContents()); }
        else super.onActivityResult(request, result, data);
    }
    public String libraryDirectory() { return getApplicationInfo().nativeLibraryDir; }
    /** Public Android APIs work for app UIDs where Go's NETLINK_ROUTE is denied. */
    public String networkSnapshot() {
        android.net.ConnectivityManager manager = (android.net.ConnectivityManager)getSystemService(CONNECTIVITY_SERVICE);
        org.json.JSONArray result = new org.json.JSONArray();
        java.util.LinkedHashSet<android.net.Network> networks = new java.util.LinkedHashSet<>();
        android.net.Network active = manager.getActiveNetwork();
        if (active != null) networks.add(active);
        java.util.Collections.addAll(networks, manager.getAllNetworks());
        java.util.HashSet<String> names = new java.util.HashSet<>();
        try {
            for (android.net.Network network : networks) {
                android.net.LinkProperties link = manager.getLinkProperties(network);
                if (link == null || link.getInterfaceName() == null || !names.add(link.getInterfaceName())) continue;
                org.json.JSONArray addresses = new org.json.JSONArray();
                for (android.net.LinkAddress address : link.getLinkAddresses()) {
                    String ip = address.getAddress().getHostAddress().split("%", 2)[0];
                    addresses.put(ip + "/" + address.getPrefixLength());
                }
                String gateway = "";
                for (android.net.RouteInfo route : link.getRoutes()) {
                    if (route.isDefaultRoute() && route.getGateway() != null) {
                        gateway = route.getGateway().getHostAddress().split("%", 2)[0]; break;
                    }
                }
                result.put(new org.json.JSONObject().put("name", link.getInterfaceName())
                    .put("mtu", link.getMtu()).put("addresses", addresses).put("gateway", gateway));
            }
            return result.toString();
        } catch (org.json.JSONException ex) { throw new IllegalStateException("Cannot read Android networks", ex); }
    }
    public String deviceName() { return (android.os.Build.MANUFACTURER + " " + android.os.Build.MODEL).substring(0, Math.min(70, (android.os.Build.MANUFACTURER + " " + android.os.Build.MODEL).length())); }
    public String openBrowser(String url) {
        if (!url.startsWith("https://login.tailscale.com/")) throw new IllegalArgumentException("Invalid login URL");
        runOnUiThread(() -> startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url))));
        return "";
    }
    private SecretKey key() throws Exception {
        KeyStore ks = KeyStore.getInstance("AndroidKeyStore"); ks.load(null);
        if (!ks.containsAlias("rtc-sleeper.session")) {
            KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
            generator.init(new KeyGenParameterSpec.Builder("rtc-sleeper.session", KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build());
            generator.generateKey();
        }
        return ((KeyStore.SecretKeyEntry)ks.getEntry("rtc-sleeper.session", null)).getSecretKey();
    }
    public String storeSession(String json) {
        try {
            if (json.isEmpty()) { getSharedPreferences("pairing", MODE_PRIVATE).edit().clear().commit(); return ""; }
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding"); cipher.init(Cipher.ENCRYPT_MODE, key());
            String data = Base64.encodeToString(cipher.getIV(), Base64.NO_WRAP) + ":" +
                Base64.encodeToString(cipher.doFinal(json.getBytes(StandardCharsets.UTF_8)), Base64.NO_WRAP);
            if (!getSharedPreferences("pairing", MODE_PRIVATE).edit().putString("session", data).commit()) throw new Exception("Storage failed");
            return "";
        } catch (Exception ex) { throw new IllegalStateException("Secure pairing storage failed", ex); }
    }
    public String loadSession() {
        try {
            String data = getSharedPreferences("pairing", MODE_PRIVATE).getString("session", "");
            if (data.isEmpty()) return "";
            String[] parts = data.split(":", 2); Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.DECRYPT_MODE, key(), new GCMParameterSpec(128, Base64.decode(parts[0], Base64.NO_WRAP)));
            return new String(cipher.doFinal(Base64.decode(parts[1], Base64.NO_WRAP)), StandardCharsets.UTF_8);
        } catch (Exception ex) { return ""; }
    }
}
