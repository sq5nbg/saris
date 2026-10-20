# Projekt: SARIS

SARIS to System Archiwizacji Raportów i Sprawozdań

## 🎯 Cel projektu

Celem projektu jest aplikacja web-owa, służąca studentom do przesyłania
sprawozdań i raportów z zajęć, które powinny być składowane w odpowiednich katalogach na dysku.
Dla użytkownika administracyjnego, powinna być możliwość przeglądania i pobierania plików.
Archiwizacja wszystkich raportów i sprawozdań konieczna jest gdyż okresowo komisja akredytacyjna
potrzebuje dostępu do wszystkich prac studentów, z których wyznaczane są oceny.

Aplikacja powinna wyświetlać formularz, w którym wpisuje się lub wybiera następujące dane:
- imię studenta 1 (tekst)
- nazwisko studenta 1 (tekst)
- email studenta 1 (tekst)
- imię studenta 2
- nazwisko studenta 2
- email studetna 2
- imie studenta 3
- nazwisko studenta 3
- email studenta 3
- Wybór przedmiotu z puli: PE, EwEF, PSM, PSW, LUKP, inne...
- hasło do przedmiotu
- nazwa - tytul cwiczenia
- wybór typu dokumentu: raport, sprawozdanie
- przycisk wybrania pliku do przesłania z komputera (uruchomienie dialogu wyboru pliku). Typ pliku to: pliki *.ipynb pochodzące z Google Colab i Jupyter Lab oraz pliki *.pdf
- jeśli hasło jest zgodne z wybranym przedmiotem, podane są dane przynajmniej jednego studenta, prawidłowy jest email, plik powinien zostać zapisany na serwerze w odpowiednim katalogu.
- katalogi na pliki powinny miec podział: pliki/przedmiot/rok/
- po przesłaniu pliku powinno się wyświetlić potwierdzenie odebrania pliku i jeśli to możliwe wysłane potwierdzenie na podane emaile studenta
- jeśli wystąpił błąd powinno się wyświetlić z jakiego powodu nie został przesłany plik i możliwość ponownego przesłania po poprawieniu błędu

## 💻 Tech Stack
- **Frontend:** Dowolny właściwy dla aplikacji
- **Backend:** Applikaca napisana w Rust, z wykorzystaniem Axum
- **Baza danych:** Aplikacja bez bazy danych najbardziej preferowana, ewentualnie dobrana do zadania, z preferencją znanych i popularnych baz opensource typu PostgreSQL alb SQLite

## 🏗️ Architektura i Konwencje
<conventions>
- **Repozytorium kodu** Stwórz lokalne repozytorium git dla plików źródłowych i przy kolejnych zmianach dodawaj pliki i wprowadzaj nowe commit'y z opisem zmian
- **Archiwizowane pliki** Archiwizowane pliki mają być w postaci plików *.pdf, *.ipynb, ewentualnie możliwość innych plików. Rozmiar pojedynczego pliku z ustawialnym limitem np. 20 MB
- **Nazewnictwo plików** Archiwizowane pliki powinny w nazwi mieć: rok-nazwa_przedmiotu-tytul_cwiczenia-nazwisko_studenta_1-typ_dokumentu-pelna_data_uploadu.pdf (albo ipynb)
- **Język kodu:** Pisz kod oraz komentarze wyłącznie w języku angielskim.
- **Styl komponentów:** Używaj asynchronicznego Rust (Tokio, Axum)
- **Pliki konfiguracyjne** Aplikacja może być konfigurowana przez plik konfiguracyjny typu toml: saris.toml
- **Logi** Aplikacja powinna logować do pliku wszystkie próby przesłań plików prawidłowe i błędne. W linii logu powinna być: data i dokładny czas, czy poprawne przesłanie (OK/ERROR), dane studenta, nazwa pliku (pełna ścieżka jeśli faktycznie przesłany), nr IP z którego nastąpiło przesłanie
</conventions>

## 🚀 Komendy i Przepływ Pracy
<commands>
- Przygotuj kod
- Skompiluj
- Przetestuj działanie
</commands>

## ⚠️ Ograniczenia i Zasady (Zawsze przestrzegaj)
- Przed modyfikacją istniejących plików zawsze upewnij się, że rozumiesz ich obecną logikę.
- Nie usuwaj istniejącej obsługi błędów ani logów bez wyraźnego polecenia.
- Jeśli wprowadzasz nową bibliotekę, najpierw poinformuj o tym użytkownika.

