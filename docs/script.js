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
    let macAsset = null;
    let linuxAsset = null;

    gReleaseData.assets.forEach(asset => {
        const name = asset.name.toLowerCase();
        // Matching rules for DataBeam binaries (e.g., databeam.exe, databeam-mac, databeam-linux)
        if (name.includes("win") || name.endsWith(".exe")) {
            windowsAsset = asset.browser_download_url;
        } else if (name.includes("mac") || name.includes("darwin")) {
            macAsset = asset.browser_download_url;
        } else if (name.includes("linux")) {
            linuxAsset = asset.browser_download_url;
        }
    });

    // If an asset wasn't found, fallback to the main release page
    const releaseUrl = gReleaseData.html_url;

    document.getElementById("link-windows").href = windowsAsset || releaseUrl;
    document.getElementById("link-mac").href = macAsset || releaseUrl;
    document.getElementById("link-linux").href = linuxAsset || releaseUrl;

    const mainBtn = document.getElementById("download-btn");
    const statusText = document.getElementById("download-status");
    const btnText = mainBtn.querySelector(".btn-text");

    let autoUrl = null;

    if (gUserOS === "windows" && windowsAsset) {
        autoUrl = windowsAsset;
        statusText.textContent = `Latest Release: ${gReleaseData.tag_name} (Windows)`;
        btnText.textContent = "Download for Windows";
    } else if (gUserOS === "mac" && macAsset) {
        autoUrl = macAsset;
        statusText.textContent = `Latest Release: ${gReleaseData.tag_name} (macOS)`;
        btnText.textContent = "Download for macOS";
    } else if (gUserOS === "linux" && linuxAsset) {
        autoUrl = linuxAsset;
        statusText.textContent = `Latest Release: ${gReleaseData.tag_name} (Linux)`;
        btnText.textContent = "Download for Linux";
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
