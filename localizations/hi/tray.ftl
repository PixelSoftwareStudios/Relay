# Relay desktop UI catalog, Hindi (hi), section: Tray menu and tooltip.
# Terms: call=कॉल, mute=म्यूट, deafen=सुनना बंद, peer=डिवाइस, channel=चैनल, invite=आमंत्रण, trust=भरोसा, file offer=फ़ाइल प्रस्ताव.
# Same ids and $variables as localizations/en/tray.ftl. See docs/i18n.md.

tray-mock = Relay (डेमो डेटा)
tray-starting = Relay: बैकग्राउंड सेवा शुरू हो रही है
tray-connecting = Relay: बैकग्राउंड सेवा से जुड़ रहा है
tray-reconnecting = Relay: बैकग्राउंड सेवा से कनेक्शन टूटा, फिर से जुड़ रहा है
tray-offline = Relay: बैकग्राउंड सेवा नहीं चल रही
tray-incoming =
    { $count ->
        [one] Relay: { $count } आने वाली कॉल
       *[other] Relay: { $count } आने वाली कॉल
    }
tray-active =
    { $count ->
        [one] Relay: { $count } सक्रिय कॉल
       *[other] Relay: { $count } सक्रिय कॉल
    }
tray-idle = Relay: निष्क्रिय
tray-show = Relay दिखाएँ
tray-quit = इस ऐप से बाहर निकलें (बैकग्राउंड सेवा चलती रहेगी)
tray-status-available = उपलब्ध
tray-status-unavailable = अनुपलब्ध
tray-stay-available = बैकग्राउंड में उपलब्ध रहें
