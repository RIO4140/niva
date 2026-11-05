# الملفات ومعالجة الأخطاء
try:
    print(10 / 0)
except:
    print("خطأ في القسمة")
with open("rio-test.txt", "w") as ملف:
    ملف.write("مرحبا من Rio4140")
with open("rio-test.txt") as ملف:
    print(ملف.read())
try:
    س = 1 / 0
except:
    print("تم الإمساك بالخطأ")
finally:
    print("انتهى")

