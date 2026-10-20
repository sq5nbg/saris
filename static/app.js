// SARIS upload form logic (vanilla JS, no dependencies).
(function () {
  "use strict";

  // Defaults used when /api/config cannot be fetched.
  var config = {
    subjects: ["PE", "EwEF", "PSM", "PSW", "LUKP"],
    max_file_size_mb: 20,
    allowed_extensions: ["pdf", "ipynb"],
    doc_types: ["raport", "sprawozdanie"]
  };

  // Simple, permissive email check (server is the authority).
  var EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

  var form = document.getElementById("upload-form");
  var subjectSelect = document.getElementById("subject");
  var fileInput = document.getElementById("file");
  var fileInfo = document.getElementById("file-info");
  var submitBtn = document.getElementById("btn-submit");
  var progressWrap = document.getElementById("progress-wrap");
  var progressBar = document.getElementById("progress");
  var progressText = document.getElementById("progress-text");
  var successPanel = document.getElementById("success-panel");
  var errorPanel = document.getElementById("error-panel");
  var errorList = document.getElementById("error-list");
  var btnAdd2 = document.getElementById("btn-add-2");
  var btnAdd3 = document.getElementById("btn-add-3");

  // ---------- helpers ----------

  function $(id) { return document.getElementById(id); }

  function val(name) {
    var el = form.elements[name];
    return el && typeof el.value === "string" ? el.value.trim() : "";
  }

  function formatSize(bytes) {
    if (bytes < 1024) return bytes + " B";
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1).replace(".", ",") + " KB";
    return (bytes / (1024 * 1024)).toFixed(2).replace(".", ",") + " MB";
  }

  function fileExtension(name) {
    var idx = name.lastIndexOf(".");
    return idx >= 0 ? name.slice(idx + 1).toLowerCase() : "";
  }

  function setFieldError(id, message) {
    var errEl = $(id + "-err");
    var target = id === "doc_type" ? $("doc_type-group") : $(id);
    if (errEl) errEl.textContent = message || "";
    if (target) {
      if (message) target.setAttribute("aria-invalid", "true");
      else target.removeAttribute("aria-invalid");
    }
  }

  function clearAllFieldErrors() {
    var errs = form.querySelectorAll(".field-error");
    for (var i = 0; i < errs.length; i++) errs[i].textContent = "";
    var invalid = form.querySelectorAll("[aria-invalid]");
    for (var j = 0; j < invalid.length; j++) invalid[j].removeAttribute("aria-invalid");
  }

  function showErrors(messages) {
    errorList.innerHTML = "";
    messages.forEach(function (m) {
      var li = document.createElement("li");
      li.textContent = m;
      errorList.appendChild(li);
    });
    errorPanel.hidden = false;
    errorPanel.focus();
    errorPanel.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  function hideErrors() {
    errorPanel.hidden = true;
    errorList.innerHTML = "";
  }

  // ---------- config ----------

  function applyConfig() {
    // Subjects
    var current = subjectSelect.value;
    while (subjectSelect.options.length > 1) subjectSelect.remove(1);
    config.subjects.forEach(function (s) {
      var opt = document.createElement("option");
      opt.value = s;
      opt.textContent = s;
      subjectSelect.appendChild(opt);
    });
    if (current && config.subjects.indexOf(current) >= 0) subjectSelect.value = current;

    // File input constraints
    var exts = config.allowed_extensions.map(function (e) {
      return "." + String(e).replace(/^\./, "").toLowerCase();
    });
    fileInput.setAttribute("accept", exts.join(","));
    $("allowed-ext").textContent = exts.join(", ");
    $("max-size").textContent = String(config.max_file_size_mb);

    // Document types (rebuild radios if the server provides a list)
    var group = $("doc_type-group");
    var checked = form.querySelector('input[name="doc_type"]:checked');
    var checkedVal = checked ? checked.value : "";
    group.innerHTML = "";
    config.doc_types.forEach(function (t) {
      var label = document.createElement("label");
      label.className = "radio";
      var input = document.createElement("input");
      input.type = "radio";
      input.name = "doc_type";
      input.value = t;
      if (t === checkedVal) input.checked = true;
      label.appendChild(input);
      label.appendChild(document.createTextNode(" " + t));
      group.appendChild(label);
    });
  }

  function loadConfig() {
    applyConfig();
    if (!window.fetch) return;
    fetch("/api/config", { headers: { "Accept": "application/json" } })
      .then(function (r) {
        if (!r.ok) throw new Error("HTTP " + r.status);
        return r.json();
      })
      .then(function (data) {
        if (data && Array.isArray(data.subjects) && data.subjects.length) config.subjects = data.subjects;
        if (data && typeof data.max_file_size_mb === "number" && data.max_file_size_mb > 0) config.max_file_size_mb = data.max_file_size_mb;
        if (data && Array.isArray(data.allowed_extensions) && data.allowed_extensions.length) config.allowed_extensions = data.allowed_extensions;
        if (data && Array.isArray(data.doc_types) && data.doc_types.length) config.doc_types = data.doc_types;
        applyConfig();
        if (fileInput.files && fileInput.files.length) updateFileInfo();
      })
      .catch(function () {
        // Keep defaults; the server still validates everything.
      });
  }

  // ---------- optional students ----------

  function showStudent(n) {
    $("student-" + n).hidden = false;
    if (n === 2) {
      btnAdd2.hidden = true;
      btnAdd3.hidden = !$("student-3").hidden;
    } else {
      btnAdd3.hidden = true;
    }
    $("s" + n + "_first").focus();
  }

  function hideStudent(n) {
    ["first", "last", "email"].forEach(function (f) {
      $("s" + n + "_" + f).value = "";
      setFieldError("s" + n + "_" + f, "");
    });
    $("student-" + n).hidden = true;
    if (n === 2) {
      // If student 3 is filled in, move it to slot 2 so numbering stays contiguous.
      var s3visible = !$("student-3").hidden;
      if (s3visible) {
        ["first", "last", "email"].forEach(function (f) {
          $("s2_" + f).value = $("s3_" + f).value;
          $("s3_" + f).value = "";
        });
        $("student-3").hidden = true;
        $("student-2").hidden = false;
        btnAdd3.hidden = false;
        btnAdd2.hidden = true;
      } else {
        btnAdd2.hidden = false;
        btnAdd3.hidden = true;
      }
      (s3visible ? btnAdd3 : btnAdd2).focus();
    } else {
      btnAdd3.hidden = false;
      btnAdd3.focus();
    }
  }

  btnAdd2.addEventListener("click", function () { showStudent(2); });
  btnAdd3.addEventListener("click", function () { showStudent(3); });
  var removeBtns = form.querySelectorAll(".btn-remove");
  for (var i = 0; i < removeBtns.length; i++) {
    removeBtns[i].addEventListener("click", function (e) {
      hideStudent(parseInt(e.currentTarget.getAttribute("data-student"), 10));
    });
  }

  // ---------- file selection ----------

  function checkFile(file) {
    if (!file) return "Wybierz plik do przesłania.";
    var allowed = config.allowed_extensions.map(function (e) { return String(e).replace(/^\./, "").toLowerCase(); });
    var ext = fileExtension(file.name);
    if (allowed.indexOf(ext) < 0) {
      return "Niedozwolony typ pliku. Dozwolone: " + allowed.map(function (e) { return "." + e; }).join(", ") + ".";
    }
    if (file.size === 0) return "Wybrany plik jest pusty.";
    if (file.size > config.max_file_size_mb * 1024 * 1024) {
      return "Plik jest za duży (" + formatSize(file.size) + "). Maksymalny rozmiar to " + config.max_file_size_mb + " MB.";
    }
    return "";
  }

  function updateFileInfo() {
    var file = fileInput.files && fileInput.files[0];
    if (!file) {
      fileInfo.textContent = "";
      setFieldError("file", "");
      return;
    }
    fileInfo.textContent = "Wybrany plik: " + file.name + " (" + formatSize(file.size) + ")";
    setFieldError("file", checkFile(file));
  }

  fileInput.addEventListener("change", updateFileInfo);

  // Clear a field's error as soon as the user edits it.
  form.addEventListener("input", function (e) {
    var t = e.target;
    if (!t || !t.name || t.type === "file") return;
    if (t.name === "doc_type") setFieldError("doc_type", "");
    else if (t.id) setFieldError(t.id, "");
  });
  form.addEventListener("change", function (e) {
    if (e.target && e.target.name === "doc_type") setFieldError("doc_type", "");
  });

  // ---------- validation ----------

  // Returns the id of the first invalid field, or "" if the form is valid.
  function validate() {
    clearAllFieldErrors();
    var firstInvalid = "";
    function fail(id, msg) {
      setFieldError(id, msg);
      if (!firstInvalid) firstInvalid = id;
    }

    // Student 1: all fields required.
    if (!val("s1_first")) fail("s1_first", "Podaj imię studenta 1.");
    if (!val("s1_last")) fail("s1_last", "Podaj nazwisko studenta 1.");
    if (!val("s1_email")) fail("s1_email", "Podaj adres e-mail studenta 1.");
    else if (!EMAIL_RE.test(val("s1_email"))) fail("s1_email", "Nieprawidłowy adres e-mail.");

    // Students 2 and 3: either fully empty or fully filled.
    [2, 3].forEach(function (n) {
      var p = "s" + n + "_";
      var first = val(p + "first"), last = val(p + "last"), email = val(p + "email");
      if (!first && !last && !email) return;
      if (!first) fail(p + "first", "Podaj imię studenta " + n + " (lub wyczyść wszystkie jego pola).");
      if (!last) fail(p + "last", "Podaj nazwisko studenta " + n + " (lub wyczyść wszystkie jego pola).");
      if (!email) fail(p + "email", "Podaj adres e-mail studenta " + n + " (lub wyczyść wszystkie jego pola).");
      else if (!EMAIL_RE.test(email)) fail(p + "email", "Nieprawidłowy adres e-mail.");
    });

    if (!val("subject")) fail("subject", "Wybierz przedmiot.");
    if (!form.elements["password"].value) fail("password", "Podaj hasło do przedmiotu.");
    if (!val("title")) fail("title", "Podaj tytuł ćwiczenia.");
    if (!form.querySelector('input[name="doc_type"]:checked')) fail("doc_type", "Wybierz typ dokumentu.");

    var fileErr = checkFile(fileInput.files && fileInput.files[0]);
    if (fileErr) fail("file", fileErr);

    return firstInvalid;
  }

  // ---------- submission ----------

  function setBusy(busy) {
    submitBtn.disabled = busy;
    submitBtn.textContent = busy ? "Przesyłanie…" : "Prześlij plik";
    form.setAttribute("aria-busy", busy ? "true" : "false");
    progressWrap.hidden = !busy;
    if (busy) {
      progressBar.value = 0;
      progressText.textContent = "0%";
    }
  }

  function messageForStatus(status) {
    if (status === 0) return "Brak połączenia z serwerem. Sprawdź połączenie z internetem i spróbuj ponownie.";
    if (status === 413) return "Plik jest za duży. Maksymalny rozmiar to " + config.max_file_size_mb + " MB.";
    if (status >= 500) return "Błąd serwera (kod " + status + "). Spróbuj ponownie później.";
    return "Nie udało się przesłać pliku (kod " + status + ").";
  }

  function parseJson(text) {
    try { return JSON.parse(text); } catch (e) { return null; }
  }

  function showSuccess(data) {
    form.hidden = true;
    hideErrors();
    $("success-message").textContent = data.message || "Plik został poprawnie zapisany w archiwum.";
    $("success-stored-name").textContent = data.stored_name || "—";
    $("success-email").textContent = data.email_sent
      ? "wysłano na podane adresy e-mail."
      : "nie zostało wysłane (wysyłka e-mail jest wyłączona lub nie powiodła się).";
    successPanel.hidden = false;
    successPanel.focus();
    successPanel.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  form.addEventListener("submit", function (e) {
    e.preventDefault();
    hideErrors();
    successPanel.hidden = true;

    var firstInvalid = validate();
    if (firstInvalid) {
      var el = firstInvalid === "doc_type"
        ? form.querySelector('input[name="doc_type"]')
        : $(firstInvalid);
      if (el) el.focus();
      return;
    }

    var fd = new FormData(form);
    // Send trimmed text values (password is sent as-is).
    ["s1_first", "s1_last", "s1_email", "s2_first", "s2_last", "s2_email",
     "s3_first", "s3_last", "s3_email", "subject", "title"].forEach(function (n) {
      fd.set(n, val(n));
    });

    var xhr = new XMLHttpRequest();
    xhr.open("POST", "/api/upload");
    xhr.setRequestHeader("Accept", "application/json");

    xhr.upload.addEventListener("progress", function (ev) {
      if (!ev.lengthComputable) return;
      var pct = Math.round((ev.loaded / ev.total) * 100);
      progressBar.value = pct;
      progressText.textContent = pct + "%";
    });

    xhr.addEventListener("load", function () {
      setBusy(false);
      var data = parseJson(xhr.responseText);
      if (xhr.status >= 200 && xhr.status < 300 && data && data.ok) {
        showSuccess(data);
        return;
      }
      var errors = data && Array.isArray(data.errors) && data.errors.length
        ? data.errors.map(String)
        : [messageForStatus(xhr.status)];
      showErrors(errors);
    });

    xhr.addEventListener("error", function () {
      setBusy(false);
      showErrors([messageForStatus(0)]);
    });

    xhr.addEventListener("timeout", function () {
      setBusy(false);
      showErrors(["Przekroczono czas oczekiwania na odpowiedź serwera. Spróbuj ponownie."]);
    });

    xhr.addEventListener("abort", function () {
      setBusy(false);
      showErrors(["Przesyłanie zostało przerwane."]);
    });

    setBusy(true);
    xhr.send(fd);
  });

  // "Send another file": keep students, subject, password and doc type; reset title and file.
  $("btn-again").addEventListener("click", function () {
    successPanel.hidden = true;
    form.elements["title"].value = "";
    fileInput.value = "";
    updateFileInfo();
    clearAllFieldErrors();
    form.hidden = false;
    form.elements["title"].focus();
  });

  loadConfig();
})();
