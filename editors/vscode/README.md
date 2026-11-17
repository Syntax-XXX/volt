# Volt für VS Code

Syntaxfarben für `.vt` und `.vts`, Fehler beim Tippen, Vervollständigung (Schlüsselwörter, Funktionen, Module, Snippets), Hover mit Signaturen, Formatieren und Befehle (**Volt: Datei ausführen**, **Tests ausführen**, **Web-Seite starten**).

Voraussetzung: das Programm `volt` ist installiert (`cargo install --path cli` im Volt-Repo). Liegt es nicht im PATH, trage den Pfad unter *Einstellungen → Volt: Pfad* ein.

Installieren: `code --install-extension volt-lang-0.1.0.vsix`. Selbst bauen: `npm install && npx @vscode/vsce package`.
