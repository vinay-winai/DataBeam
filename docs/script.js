const API_URL = "https://api.github.com/repos/vinay-winai/DataBeam/releases/latest";

let gReleaseData = null;
let gUserOS = "unknown";
let gUserArch = null;
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
    if (userAgent.includes("android") || window.navigator.userAgentData?.platform === "Android") {
        return "android";
    } else if (userAgent.includes("win")) {
        return "windows";
    } else if (userAgent.includes("mac")) {
        return "mac";
    } else if (userAgent.includes("linux") || userAgent.includes("x11")) {
        return "linux";
    }
    return "unknown";
}

function detectUserAgentArch(userAgent) {
    if (/\b(aarch64|arm64)\b/i.test(userAgent)) return "arm64";
    // Chromium freezes Linux's UA/platform to x86_64 even on other CPUs.
    // Use its architecture hints instead; a denied hint needs a manual choice.
    if (/(Chrome|Chromium|Edg|OPR)\//i.test(userAgent)) return null;
    if (/\b(x86_64|amd64|x64)\b/i.test(userAgent)) return "x64";
    return null;
}

function detectHintArch(cpu) {
    if (cpu.bitness !== "64") return null;
    const architecture = (cpu.architecture || "").toLowerCase();
    if (["arm", "arm64", "aarch64"].includes(architecture)) return "arm64";
    if (["x86", "x86_64", "amd64", "x64"].includes(architecture)) return "x64";
    return null;
}

async function setupOSLinks() {
    gUserOS = detectOS();
    gUserArch = detectUserAgentArch(window.navigator.userAgent);
    
    // Set fallback GitHub page link if JS fails or API fails
    const mainBtn = document.getElementById("download-btn");
    mainBtn.href = "https://github.com/vinay-winai/DataBeam/releases/latest";
    // Some browsers hide the CPU in the user agent. Use client hints when
    // available; explicit architecture links remain available in all browsers.
    if (["linux", "android"].includes(gUserOS) && window.navigator.userAgentData?.getHighEntropyValues) {
        try {
            const cpu = await window.navigator.userAgentData.getHighEntropyValues(["architecture", "bitness"]);
            gUserArch = detectHintArch(cpu);
        } catch (_) { /* Keep only a usable, explicit user-agent architecture. */ }
    }
    updateDownloadLinks();
}

function updateDownloadLinks() {
    if (!gReleaseData || !gReleaseData.assets) return;

    let windowsAsset = null;
    let macUnivAsset = null;
    let macAarchAsset = null;
    const linuxAssets = { x64: null, arm64: null };

    gReleaseData.assets.forEach(asset => {
        const name = asset.name.toLowerCase();
        // Matching rules for DataBeam binaries
        if (name.endsWith(".exe")) {
            if (!windowsAsset || name === "databeam_windows-x64.exe") windowsAsset = asset.browser_download_url;
        } else if (name.includes("macos-universal")) {
            if (!macUnivAsset || name === "databeam_macos-universal.dmg") macUnivAsset = asset.browser_download_url;
        } else if (name.includes("macos-aarch64")) {
            if (!macAarchAsset || name === "databeam_macos-aarch64.dmg") macAarchAsset = asset.browser_download_url;
        } else if (name.includes("mac") || name.includes("darwin")) {
            if (!macUnivAsset) macUnivAsset = asset.browser_download_url;
        } else {
            const arch = name.includes("linux-arm64") ? "arm64" : name.includes("linux-x64") ? "x64" : null;
            if (arch && name.endsWith(".appimage")) {
                const stableName = `databeam_linux-${arch}.appimage`;
                if (!linuxAssets[arch] || name === stableName) {
                    linuxAssets[arch] = asset.browser_download_url;
                }
            }
        }
    });

    // If an asset wasn't found, fallback to the main release page
    const releaseUrl = gReleaseData.html_url;

    document.getElementById("link-windows").href = windowsAsset || releaseUrl;
    document.getElementById("link-mac").href = macUnivAsset || macAarchAsset || releaseUrl;
    document.getElementById("link-linux").href = linuxAssets.x64 || releaseUrl;
    document.getElementById("link-linux-arm64").href = linuxAssets.arm64 || releaseUrl;
    const linuxAppAsset = linuxAssets[gUserArch];

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
    } else if (["linux", "android"].includes(gUserOS)) {
        if (linuxAppAsset) {
            autoUrl = linuxAppAsset;
            const archLabel = gUserArch === "arm64" ? "AArch64" : "x64";
            statusText.textContent = `Latest Release: ${gReleaseData.tag_name} (Linux ${archLabel})`;
            btnText.textContent = `Download Linux ${archLabel} AppImage`;
        } else if (!gUserArch) {
            autoUrl = "#platform-downloads";
            statusText.textContent = "Choose your Linux architecture below.";
            btnText.textContent = "Choose Linux architecture";
        } else {
            autoUrl = releaseUrl;
            statusText.textContent = `Latest Release: ${gReleaseData.tag_name} (Linux)`;
            btnText.textContent = "View Linux downloads";
        }
        if (gUserOS === "android" && secStatus) {
            secStatus.style.display = "block";
            secStatus.textContent = "For DroidDeck or another Linux environment on Android.";
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
