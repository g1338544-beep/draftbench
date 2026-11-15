# DraftNew — สรุปงานสำหรับแชท/ผู้ช่วยคนถัดไป (อัปเดต 2026-11-15 19:30)

เจ้าของงาน: วิศวกรเครื่องกลชาวไทย คุยภาษาไทยเสมอ ทำงานตามลำดับที่สั่ง (ข้อความกลางงาน = เพิ่มคิว ไม่ใช่สั่งหยุด)
Repo: g1338544-beep/draftbench — `main` = ขึ้นเว็บแล้ว, `wip-relations` = งานที่ยังไม่เสร็จ
เว็บ: https://g1338544-beep.github.io/draftbench/ (ดู sha มุมขวาบน)

## ขึ้นเว็บแล้ว (main)
- Spring (สปริงยืดหดตามชิ้น), Roller on Surface, Part Feeder, Seamless Sew
- ปุ่ม AI บนเว็บ (ใช้คีย์ Anthropic ของผู้ใช้เอง เก็บในเบราว์เซอร์)
- Drawing: Auto balloons, Explode video
- Wedge Cam (`joint.wedge`): ลิ่ม/กรวยดันลูกกลิ้งบนแขน
- Air Cylinder: ก้านไม่หลุดจากกระบอก, กระบอกกลวงมีลูกสูบ (ไม่ซ้อนกัน)
- แก้: แอป PC คอมไพล์ไม่ผ่าน (Services ai_post/ai_key) — b2e7f82

## ขึ้น main รอบล่าสุด (d5aec40)
- Drawing แบบประกอบ: เส้นเซ็นเตอร์จากแกนจริงของแต่ละชิ้น (ไม่ต่อข้ามชิ้น ไม่โชว์ชิ้นที่ถูกบัง), เส้นตัดขึ้นวิวเดียว,
  Auto balloons ครบทุกชิ้น (ชิ้นซ้อนใน subassembly ด้วย) เรียงแถว เส้นไม่ไขว้, BOM แบบ Compact (bom_row),
  หมายเหตุ 3 บรรทัด, PDF หมุนตัวเลขมิติแนวตั้ง, ภาพ Exploded ในแผ่นแบบ
- แผ่น Drawing บันทึกในไฟล์ .draftnew (property com.draftnew.drawing)
- มือถือ/จอสัมผัส: กดค้าง 0.85 วิ = คลิกขวา (long_press.rs)
- ตัวอย่าง Centering Gripper เป็น assembly จริง ทุกชิ้นเป็น Component มี Part No./วัสดุ + แบบ A3 (เทส ignored make_gripper_assembly_drawing)

## เทสที่พังมาก่อน (ไม่ใช่งานรอบนี้)
- engine view_dxf::layers_off_leaves_lines_out_of_the_export_only
- ui drawing_workspace::an_angled_section_view_keeps_to_its_axis
- kernel surface_extend::hostile_requests_are_refused, surfaces::extend_a_planar_surface (เทสเก่าหลังเปลี่ยนให้ระยะติดลบย่อผิว)

## อยู่ใน wip-relations (ยังไม่ขึ้น main)
- Wedge Cam: ไม่ต้องคลิกลูกกลิ้ง (หาให้เอง), แก้กรวยที่ทำจาก Revolve แล้วแขนข้างหนึ่งหมุนผิดทาง (เทส `a_revolved_cone_turns_levers_on_both_sides_alike`)
- สปริงออกในรูป/วิดีโอที่ export (`engine/src/view.rs`)
- My Parts (คลังไฟล์ STEP/STL ของผู้ใช้ใน Insert Part) — รับต่อมาจาก branch `my-parts` (4e6a935)
- ตัวสร้างตัวอย่าง Centering Gripper: เทส ignored `make_centering_gripper_example` (env `GRIPPER_OUT`, `GRIPPER_PARTS_ONLY`)
- วิดีโอสอนใช้โปรแกรม: `examples/demo/gripper_tutorial.py` + workflow "Draftbench tutorial" input `gripper`

## คิวงาน (เจ้าของบอกให้รอ จะสั่งงานใหม่เอง 19:27)
- มือถือ: หน้าต่างคำสั่งบนจอเล็กใส่ค่ายาก (รอภาพหน้าจอจากเจ้าของ) — ไอเดีย: dialog เป็นแผ่นเต็มกว้างจากล่าง ไม่ย่อ
- (ถามเฉยๆ ไม่ต้องทำ) กระบอกลมเหลี่ยม compact: โหลด STEP ผู้ผลิตผ่านเบราว์เซอร์ในแอป (เจ้าของล็อกอินเอง) หรือทำคำสั่งสร้างจากแคตตาล็อก
- Windows build d5aec40 กำลังรัน (workflow Draftbench Windows)

## คิวงานเดิม
1. วิดีโอสอนใช้โปรแกรม (Centering Gripper) — รันบน GitHub Actions
2. My Parts: เทส ui-egui + ขึ้น main + เช็ก build/deploy + ลองในแอปจริง
3. รวม wip-relations ขึ้น main
4. เวอร์ชัน PC (Windows) — workflow "Draftbench Windows"

## ไฟล์ตัวอย่างที่เคยส่งให้ (สร้างใหม่ได้จากเทส ignored)
Wedge-Gripper, Internal-Expansion (98 ชิ้น ไม่ซ้อนกัน), Centering-Gripper (43 ชิ้น ไม่ซ้อนกัน, หนีบที่ระยะชัก 11.4 มม.)

## วิธีทำงานบนเครื่องนี้
- เครื่อง 2 คอร์: คอมไพล์ทีละงาน, งานหนักย้ายไป GitHub Actions ("Draftbench tests", "Draftbench tutorial")
- ดูวิธี build/เทสใน skill `draftnew-cad-dev` และ AGENTS.md
