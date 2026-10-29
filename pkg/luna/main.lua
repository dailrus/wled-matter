-- Вкладка «WLED → Яндекс» в панели Luna. Сам мост — отдельный процесс (bin/wled_matterd,
-- порт 8097); страница обращается к его API напрямую, Luna её только показывает.
-- register.customweb есть не во всех версиях Luna (в 0.2.3 ещё нет): без неё плагину
-- делать нечего, интерфейс остаётся доступен на порту 8097.
if not (register and register.customweb) then
  print("Эта версия Luna не умеет вкладки плагинов; интерфейс моста: http://<станция>:8097")
  return
end

local url = register.customweb("index.html")
print("WLED → Яндекс: " .. url)

while true do
  alice.sleep(60000)
end
