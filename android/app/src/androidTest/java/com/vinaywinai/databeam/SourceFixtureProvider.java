package com.vinaywinai.databeam;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.res.AssetFileDescriptor;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.DocumentsContract;
import android.provider.OpenableColumns;
import java.io.*;
import java.util.*;

/** Separate UID/private paths. Pure Java because the standalone test APK has no Kotlin runtime. */
public class SourceFixtureProvider extends ContentProvider {
    private static volatile boolean blocked;
    @Override public boolean onCreate() { return true; }
    private String node(Uri uri) {
        List<String> parts = uri.getPathSegments();
        int document = parts.indexOf("document");
        return document >= 0 ? parts.get(document + 1) : parts.get(parts.size() - 1);
    }
    private boolean directory(String id) { return Arrays.asList("root", "empty", "nested").contains(id); }
    private String name(String id) {
        switch(id) {
            case "root": return "source-folder";
            case "empty": return "empty directory";
            case "nested": return "nested";
            case "large": return "large-source.bin";
            case "slice": return "bounded.bin";
            case "pipe": return "stream.bin";
            default: return "original-name.bin";
        }
    }
    private int bytes(String id) { return id.equals("large") ? 8 * 1024 * 1024 : 65536; }
    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] args, String sort) {
        String id = node(uri);
        String[] columns = projection != null ? projection : new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE};
        List<String> rows = Collections.singletonList(id);
        if ("children".equals(uri.getLastPathSegment())) {
            rows = id.equals("root") ? Arrays.asList("regular", "nested", "empty") :
                id.equals("nested") ? Arrays.asList("pipe", "slice") : Collections.emptyList();
        }
        MatrixCursor cursor = new MatrixCursor(columns);
        for (String row : rows) {
            Object[] values = new Object[columns.length];
            for (int i = 0; i < columns.length; i++) {
                switch(columns[i]) {
                    case DocumentsContract.Document.COLUMN_DOCUMENT_ID: values[i] = row; break;
                    case OpenableColumns.DISPLAY_NAME: values[i] = name(row); break;
                    case OpenableColumns.SIZE: values[i] = bytes(row); break;
                    case DocumentsContract.Document.COLUMN_MIME_TYPE:
                        values[i] = directory(row) ? DocumentsContract.Document.MIME_TYPE_DIR : "application/octet-stream"; break;
                }
            }
            cursor.addRow(values);
        }
        return cursor;
    }
    @Override public String getType(Uri uri) {
        return directory(node(uri)) ? DocumentsContract.Document.MIME_TYPE_DIR : "application/octet-stream";
    }
    private synchronized File original(String id) throws IOException {
        File file = new File(getContext().getFilesDir(), "source-fixture-" + id + ".dat");
        boolean slice = id.equals("slice");
        if (!file.exists()) try (OutputStream out = new FileOutputStream(file)) {
            if (slice) { byte[] prefix = new byte[4096]; Arrays.fill(prefix, (byte)99); out.write(prefix); }
            byte[] block = new byte[65536]; new Random(7).nextBytes(block);
            for (int i = 0; i < bytes(id) / block.length; i++) out.write(block);
            if (slice) { byte[] suffix = new byte[4096]; Arrays.fill(suffix, (byte)55); out.write(suffix); }
        }
        return file;
    }
    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        if (!mode.equals("r")) throw new FileNotFoundException("Read only");
        if (blocked) throw new FileNotFoundException("Fixture original is unavailable");
        String id = node(uri);
        try {
            if (id.equals("pipe")) {
                ParcelFileDescriptor[] pipes = ParcelFileDescriptor.createPipe();
                new Thread(() -> {
                    try (InputStream input = new FileInputStream(original("pipe"));
                         OutputStream output = new ParcelFileDescriptor.AutoCloseOutputStream(pipes[1])) {
                        byte[] block = new byte[65536]; int count;
                        while ((count = input.read(block)) >= 0) output.write(block, 0, count);
                    } catch (IOException ignored) { }
                }).start();
                return pipes[0];
            }
            return ParcelFileDescriptor.open(original(id), ParcelFileDescriptor.MODE_READ_ONLY);
        } catch (IOException error) { throw new FileNotFoundException(error.toString()); }
    }
    @Override public AssetFileDescriptor openAssetFile(Uri uri, String mode) throws FileNotFoundException {
        String id = node(uri);
        return new AssetFileDescriptor(openFile(uri, mode), id.equals("slice") ? 4096 : 0,
            id.equals("pipe") ? AssetFileDescriptor.UNKNOWN_LENGTH : bytes(id));
    }
    @Override public Uri insert(Uri uri, ContentValues values) { throw new UnsupportedOperationException(); }
    @Override public int delete(Uri uri, String selection, String[] args) {
        blocked = !"reset".equals(uri.getLastPathSegment());
        return 0;
    }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] args) { return 0; }
}
