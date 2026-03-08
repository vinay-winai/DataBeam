const API_URL = "https://api.github.com/repos/vinay-winai/DataBeam/releases/latest";

let gReleaseData = null;
let gUserOS = "unknown";
let gAutoDownloadUrl = null;

async function fetchLatestRelease() {
    try {
        const response = await fetch(API_URL);
        if (!response.ok) throw new Error("Failed to fetch GitHub API");
        const data = await response.json();
        gReleaseData = data;
        
        // Update version badge if exists
        const badge = document.getElementById("version-badge");
        if (badge && data.tag_name) {
            badge.textContent = `${data.tag_name} Now Available`;
        }
        
        updateDownloadLinks();
    } catch (error) {
        console.error("Error fetching release:", error);
        document.getElementById("download-status").textContent = "Could not fetch latest release. Check GitHub.";
        document.getElementById("download-status").style.color = "#ff5f56";
    }
}

function detectOS() {
    const userAgent = window.navigator.userAgent.toLowerCase();
    if (userAgent.includes("win")) {
        return "windows";
    } else if (userAgent.includes("mac")) {
        return "mac";
    } else if (userAgent.includes("linux") || userAgent.includes("x11")) {
        return "linux";
    }
    return "unknown";
}

function setupOSLinks() {
    gUserOS = detectOS();
    
    // Set fallback GitHub page link if JS fails or API fails
    const mainBtn = document.getElementById("download-btn");
    mainBtn.href = "https://github.com/vinay-winai/DataBeam/releases/latest";
}

function updateDownloadLinks() {
    if (!gReleaseData || !gReleaseData.assets) return;

    let windowsAsset = null;
    let macUnivAsset = null;
    let macAarchAsset = null;
    let linuxAppAsset = null;
    let linuxDebAsset = null;

    gReleaseData.assets.forEach(asset => {
        const name = asset.name.toLowerCase();
        // Matching rules for DataBeam binaries
        if (name.includes("win") || name.endsWith(".exe")) {
            windowsAsset = asset.browser_download_url;
        } else if (name.includes("macos-universal")) {
            macUnivAsset = asset.browser_download_url;
        } else if (name.includes("macos-aarch64")) {
            macAarchAsset = asset.browser_download_url;
        } else if (name.includes("mac") || name.includes("darwin")) {
            if (!macUnivAsset) macUnivAsset = asset.browser_download_url;
        } else if (name.endsWith(".appimage")) {
            linuxAppAsset = asset.browser_download_url;
        } else if (name.endsWith(".deb")) {
            linuxDebAsset = asset.browser_download_url;
        } else if (name.includes("linux")) {
             if (!linuxAppAsset) linuxAppAsset = asset.browser_download_url;
        }
    });

    // If an asset wasn't found, fallback to the main release page
    const releaseUrl = gReleaseData.html_url;

    document.getElementById("link-windows").href = windowsAsset || releaseUrl;
    document.getElementById("link-mac").href = macUnivAsset || macAarchAsset || releaseUrl;
    document.getElementById("link-linux").href = linuxAppAsset || linuxDebAsset || releaseUrl;

    const mainBtn = document.getElementById("download-btn");
    const statusText = document.getElementById("download-status");
    const btnText = mainBtn.querySelector(".btn-text");

    let autoUrl = null;
    const secStatus = document.getElementById("secondary-download");
    if (secStatus) {
        secStatus.style.display = "none";
        secStatus.innerHTML = "";
    }

    if (gUserOS === "windows" && windowsAsset) {
        autoUrl = windowsAsset;
        statusText.textContent = `Latest Release: ${gReleaseData.tag_name} (Windows)`;
        btnText.textContent = "Download for Windows";
    } else if (gUserOS === "mac" && (macUnivAsset || macAarchAsset)) {
        autoUrl = macUnivAsset || macAarchAsset;
        statusText.textContent = `Latest Release: ${gReleaseData.tag_name} (macOS Universal)`;
        btnText.textContent = "Download for macOS";
        if (secStatus && macAarchAsset) {
            secStatus.style.display = "block";
            secStatus.innerHTML = `On Apple Silicon? <a href="${macAarchAsset}" style="color: #ff8c00; text-decoration: underline;">Get aarch64 build for smaller app size</a>`;
        }
    } else if (gUserOS === "linux" && (linuxAppAsset || linuxDebAsset)) {
        autoUrl = linuxAppAsset || linuxDebAsset;
        statusText.textContent = `Latest Release: ${gReleaseData.tag_name} (Linux)`;
        btnText.textContent = autoUrl === linuxAppAsset ? "Download AppImage" : "Download Linux dist";
        if (secStatus && linuxDebAsset && autoUrl !== linuxDebAsset) {
            secStatus.style.display = "block";
            secStatus.innerHTML = `Also available: <a href="${linuxDebAsset}" style="color: #ff8c00; text-decoration: underline;">Download .deb package</a>`;
        } else if (secStatus && linuxAppAsset && autoUrl !== linuxAppAsset) {
            secStatus.style.display = "block";
            secStatus.innerHTML = `Also available: <a href="${linuxAppAsset}" style="color: #ff8c00; text-decoration: underline;">Download AppImage</a>`;
        }
    } else {
        autoUrl = releaseUrl; // fallback
        statusText.textContent = `Latest Release: ${gReleaseData.tag_name} (Cross-Platform)`;
        btnText.textContent = "View Latest Release";
    }

    mainBtn.href = autoUrl;
    gAutoDownloadUrl = autoUrl;
}

// Initialize
document.addEventListener("DOMContentLoaded", () => {
    setupOSLinks();
    fetchLatestRelease();
});
