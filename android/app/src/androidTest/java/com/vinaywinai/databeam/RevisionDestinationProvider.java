package com.vinaywinai.databeam;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.provider.DocumentsContract;
import java.io.File;
import java.io.FileNotFoundException;
import java.io.IOException;

/** A writable test document tree containing only generated receive fixtures. */
public class RevisionDestinationProvider extends ContentProvider {
    @Override public boolean onCreate() { return true; }

    private String documentId(Uri uri) {
        return DocumentsContract.getDocumentId(uri);
    }

    private File file(Uri uri) {
        String tree = DocumentsContract.getTreeDocumentId(uri);
        String id = documentId(uri);
        if (!tree.matches("revision-[0-9a-f-]{36}") || !(id.equals(tree) || id.startsWith(tree + "/"))) {
            throw new IllegalArgumentException("Invalid fixture URI");
        }
        File treeRoot = new File(getContext().getCacheDir(), tree);
        treeRoot.mkdirs();
        File result = new File(getContext().getCacheDir(), id);
        try {
            if (!result.getCanonicalFile().toPath().startsWith(treeRoot.getCanonicalFile().toPath())) {
                throw new IllegalArgumentException("Outside the fixture tree");
            }
        } catch (IOException error) {
            throw new IllegalArgumentException(error);
        }
        return result;
    }

    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] args, String sort) {
        MatrixCursor cursor = new MatrixCursor(projection);
        File file = file(uri);
        String id = documentId(uri);
        if ("children".equals(uri.getLastPathSegment())) {
            File[] children = file.listFiles();
            if (children != null) {
                for (File child : children) addRow(cursor, projection, id + "/" + child.getName(), child);
            }
        } else if (file.exists()) {
            addRow(cursor, projection, id, file);
        }
        return cursor;
    }

    private void addRow(MatrixCursor cursor, String[] columns, String id, File file) {
        Object[] values = new Object[columns.length];
        for (int i = 0; i < columns.length; i++) {
            switch (columns[i]) {
                case DocumentsContract.Document.COLUMN_DOCUMENT_ID: values[i] = id; break;
                case DocumentsContract.Document.COLUMN_DISPLAY_NAME: values[i] = file.getName(); break;
                case DocumentsContract.Document.COLUMN_MIME_TYPE: values[i] = type(file); break;
                case DocumentsContract.Document.COLUMN_SIZE: values[i] = file.length(); break;
            }
        }
        cursor.addRow(values);
    }

    private String type(File file) {
        return file.isDirectory() ? DocumentsContract.Document.MIME_TYPE_DIR : "application/octet-stream";
    }

    @Override public Bundle call(String method, String arg, Bundle extras) {
        if ("android:createDocument".equals(method)) {
            Uri parent = extras.getParcelable("uri");
            String name = extras.getString(DocumentsContract.Document.COLUMN_DISPLAY_NAME);
            if (name == null || name.equals(".") || name.equals("..") || name.contains("/") || name.contains("\\")) {
                throw new IllegalArgumentException("Invalid fixture filename");
            }
            File target = new File(file(parent), name);
            boolean directory = DocumentsContract.Document.MIME_TYPE_DIR.equals(
                extras.getString(DocumentsContract.Document.COLUMN_MIME_TYPE));
            try {
                if (!(directory ? target.mkdir() : target.createNewFile())) {
                    throw new IllegalArgumentException("Fixture name already exists");
                }
            } catch (IOException error) {
                throw new IllegalArgumentException(error);
            }
            Bundle result = new Bundle();
            result.putParcelable("uri", DocumentsContract.buildDocumentUriUsingTree(parent, documentId(parent) + "/" + name));
            return result;
        }
        if ("android:deleteDocument".equals(method)) {
            file(extras.getParcelable("uri")).delete();
            return new Bundle();
        }
        return super.call(method, arg, extras);
    }

    @Override public String getType(Uri uri) { return type(file(uri)); }
    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        return ParcelFileDescriptor.open(file(uri), ParcelFileDescriptor.parseMode(mode));
    }
    @Override public Uri insert(Uri uri, ContentValues values) { throw new UnsupportedOperationException(); }
    @Override public int delete(Uri uri, String selection, String[] args) { return file(uri).delete() ? 1 : 0; }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] args) { return 0; }
}
