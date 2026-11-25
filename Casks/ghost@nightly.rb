# Ghost as main is now: the rolling `nightly` prerelease, replaced on each
# push by .github/workflows/app.yml.
cask "ghost@nightly" do
  version :latest
  sha256 :no_check

  url "https://github.com/u0-code/u0/releases/download/nightly/Ghost-macos-arm64.zip"
  name "Ghost Nightly"
  desc "Desktop app for the coding agents you already use"
  homepage "https://github.com/u0-code/u0"

  depends_on arch: :arm64
  depends_on macos: :ventura

  app "Ghost.app"

  # Ghost is signed ad hoc, not notarized, and Gatekeeper refuses to open
  # it while it is quarantined: installing this cask is the choice to open it.
  postflight_steps do
    run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{appdir}}/Ghost.app"],
                          writable_paths: ["Ghost.app"], writable_base: :appdir
  end

  zap trash: "~/.ghost"

  caveats "Ghost is not notarized; this cask removed the quarantine from Ghost.app so that it opens."
end
