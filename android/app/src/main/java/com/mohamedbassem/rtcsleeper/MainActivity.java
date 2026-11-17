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
    @Override protected void onCreate(Bundle state) {
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
    }
    public String scanQr() {
        runOnUiThread(() -> new IntentIntegrator(this)
            .setDesiredBarcodeFormats(IntentIntegrator.QR_CODE)
            .setPrompt("Scan the QR code in RTC Sleeper on your laptop")
            .setBeepEnabled(false).setOrientationLocked(false).initiateScan());
        return "";
    }
    @Override protected void onActivityResult(int request, int result, Intent data) {
        IntentResult scan = IntentIntegrator.parseActivityResult(request, result, data);
        if (scan != null) { if (scan.getContents() != null) nativeScan(scan.getContents()); }
        else super.onActivityResult(request, result, data);
    }
    public String libraryDirectory() { return getApplicationInfo().nativeLibraryDir; }
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
